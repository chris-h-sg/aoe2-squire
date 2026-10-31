//! Dev-only screen recording (`--record-video <dir>`, `dev-tools` feature): ffmpeg records
//! the primary monitor while a match is captured, so the telemetry can be checked against
//! a video of the game without starting, stopping or cutting a recording by hand.
//!
//! ffmpeg (6.0 or newer, for the `ddagrab` filter) captures DXGI output 0, the monitor
//! the capture loop reads, and encodes on the GPU where possible. Each match gets
//! `<dir>/telemetry_<start>.mkv` (Matroska stays playable if ffmpeg is killed), a
//! `.video.json` sidecar with the wall-clock time ffmpeg was started, and ffmpeg's log.
//!
//! The capture loop only spawns and stops the process; it never waits on ffmpeg while
//! frames are being read. The running recording is kept in a global so the Ctrl+C handler
//! can stop it before the process exits.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEFAULT_FPS: u32 = 30;
/// How long ffmpeg gets to finish the file after `q` before it is killed.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// Environment variable naming the ffmpeg binary when `--ffmpeg` is not given.
pub const FFMPEG_ENV: &str = "SQUIRE_FFMPEG";
/// The monitor captured by `DxgiSource` (`EnumOutputs(0)` on the default adapter).
const OUTPUT_IDX: u32 = 0;
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoder {
    Nvenc,
    Amf,
    Qsv,
    /// CPU fallback; costs far more CPU than the GPU encoders.
    X264,
}

impl Encoder {
    /// Tried in this order when no encoder is configured.
    pub const ALL: [Encoder; 4] = [Encoder::Nvenc, Encoder::Amf, Encoder::Qsv, Encoder::X264];

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Encoder::Nvenc => "nvenc",
            Encoder::Amf => "amf",
            Encoder::Qsv => "qsv",
            Encoder::X264 => "x264",
        }
    }

    /// The ffmpeg encoder this choice uses, as listed by `ffmpeg -encoders`.
    pub fn codec(self) -> &'static str {
        match self {
            Encoder::Nvenc => "h264_nvenc",
            Encoder::Amf => "h264_amf",
            Encoder::Qsv => "h264_qsv",
            Encoder::X264 => "libx264",
        }
    }

    /// Filter and encoder arguments. `ddagrab` delivers D3D11 frames on the GPU: NVENC and
    /// AMF take them directly, QuickSync needs them mapped, x264 needs them downloaded.
    /// The quality settings keep the resource panel digits readable.
    fn args(self) -> &'static [&'static str] {
        match self {
            Encoder::Nvenc => &[
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p4",
                "-rc",
                "vbr",
                "-cq",
                "23",
                "-b:v",
                "0",
            ],
            Encoder::Amf => &[
                "-c:v", "h264_amf", "-quality", "speed", "-rc", "cqp", "-qp_i", "23", "-qp_p", "23",
            ],
            Encoder::Qsv => &[
                "-vf",
                "hwmap=derive_device=qsv,format=qsv",
                "-c:v",
                "h264_qsv",
                "-preset",
                "veryfast",
                "-global_quality",
                "23",
            ],
            Encoder::X264 => &[
                "-vf",
                "hwdownload,format=bgra,format=yuv420p",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "23",
            ],
        }
    }
}

/// ffmpeg input and encoder arguments: capture output 0 at `fps` frames per second (frames
/// are repeated when the screen does not change) and encode with a keyframe every 2 s, so
/// seeking in the video stays fast, and without B-frames (with them, QuickSync writes
/// non-monotonic timestamps at the start).
pub fn capture_args(encoder: Encoder, fps: u32) -> Vec<String> {
    let mut args = vec![
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        format!("ddagrab=output_idx={}:framerate={}", OUTPUT_IDX, fps),
    ];
    args.extend(encoder.args().iter().map(|s| s.to_string()));
    args.extend(["-bf".to_string(), "0".to_string()]);
    args.extend(["-g".to_string(), (fps * 2).to_string()]);
    args
}

/// The full ffmpeg arguments for recording a match to `video` (Matroska).
pub fn record_args(encoder: Encoder, fps: u32, video: &Path) -> Vec<OsString> {
    ["-hide_banner", "-nostats", "-loglevel", "warning", "-y"]
        .iter()
        .map(OsString::from)
        .chain(capture_args(encoder, fps).into_iter().map(OsString::from))
        .chain(["-f".into(), "matroska".into(), video.into()])
        .collect()
}

#[derive(Clone, Debug)]
pub struct VideoOptions {
    pub dir: PathBuf,
    pub ffmpeg: PathBuf,
    pub encoder: Encoder,
    pub fps: u32,
}

impl VideoOptions {
    /// Checks that `ffmpeg` can record the screen and picks the encoder: the given one, or
    /// the first of `Encoder::ALL` that works on this machine (each is tried with a short
    /// test capture). Errors explain what is missing.
    pub fn resolve(
        dir: PathBuf,
        ffmpeg: PathBuf,
        encoder: Option<Encoder>,
        fps: u32,
    ) -> Result<Self, String> {
        check_ffmpeg(&ffmpeg)?;
        let encoders = run_ffmpeg(&ffmpeg, &["-hide_banner", "-encoders"])
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .map_err(|e| format!("could not run {}: {}", ffmpeg.display(), e))?;
        let candidates: Vec<Encoder> = match encoder {
            Some(e) => vec![e],
            None => Encoder::ALL.to_vec(),
        };
        let mut failures = Vec::new();
        for candidate in candidates {
            if !lists_encoder(&encoders, candidate.codec()) {
                failures.push(format!("{}: not in this ffmpeg build", candidate.codec()));
                continue;
            }
            match probe_encoder(&ffmpeg, candidate, fps) {
                Ok(()) => {
                    return Ok(Self {
                        dir,
                        ffmpeg,
                        encoder: candidate,
                        fps,
                    })
                }
                Err(e) => failures.push(format!("{}: {}", candidate.codec(), e)),
            }
        }
        Err(format!(
            "no working video encoder found with {}:\n  {}",
            ffmpeg.display(),
            failures.join("\n  ")
        ))
    }
}

/// The ffmpeg binary: `--ffmpeg <path>`, else `$SQUIRE_FFMPEG`, else `ffmpeg` on PATH.
pub fn ffmpeg_path(flag: Option<&str>) -> PathBuf {
    flag.map(PathBuf::from)
        .or_else(|| std::env::var_os(FFMPEG_ENV).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

fn check_ffmpeg(ffmpeg: &Path) -> Result<(), String> {
    let version = run_ffmpeg(ffmpeg, &["-hide_banner", "-version"]).map_err(|e| {
        format!(
            "could not run {} ({}). Install ffmpeg 6.0 or newer (e.g. `winget install \
             Gyan.FFmpeg`) and pass --ffmpeg <path> or set {}.",
            ffmpeg.display(),
            e,
            FFMPEG_ENV
        )
    })?;
    let filters = run_ffmpeg(ffmpeg, &["-hide_banner", "-filters"])
        .map_err(|e| format!("could not run {}: {}", ffmpeg.display(), e))?;
    check_capabilities(
        &String::from_utf8_lossy(&version.stdout),
        &String::from_utf8_lossy(&filters.stdout),
    )
    .map_err(|e| format!("{} ({})", e, ffmpeg.display()))
}

/// Checks `ffmpeg -version` and `ffmpeg -filters` output for screen capture support.
pub fn check_capabilities(version: &str, filters: &str) -> Result<(), String> {
    let has_ddagrab = filters
        .lines()
        .any(|l| l.split_whitespace().nth(1) == Some("ddagrab"));
    if has_ddagrab {
        return Ok(());
    }
    let found = match parse_version(version) {
        Some((major, minor)) => format!("ffmpeg {}.{}", major, minor),
        None => "this ffmpeg".to_string(),
    };
    Err(format!(
        "{} has no ddagrab screen capture filter; it needs ffmpeg 6.0 or newer built for \
         Windows (e.g. `winget install Gyan.FFmpeg`). Pass its path with --ffmpeg <path> or \
         set {}",
        found, FFMPEG_ENV
    ))
}

/// Major and minor version from `ffmpeg -version`, e.g. "ffmpeg version 7.1-full_build..."
/// gives (7, 1). Git builds ("ffmpeg version N-112233-g...") have no version number.
pub fn parse_version(version: &str) -> Option<(u32, u32)> {
    let word = version
        .split_whitespace()
        .skip_while(|w| *w != "version")
        .nth(1)?;
    let mut parts = word.split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

/// Whether `ffmpeg -encoders` output lists `codec`, e.g. " V....D h264_nvenc  NVIDIA...".
pub fn lists_encoder(encoders: &str, codec: &str) -> bool {
    encoders
        .lines()
        .any(|l| l.split_whitespace().nth(1) == Some(codec))
}

fn run_ffmpeg(ffmpeg: &Path, args: &[&str]) -> io::Result<Output> {
    Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .output()
}

/// Captures and encodes a few frames to nowhere, to find out whether the encoder works
/// with this GPU and monitor.
fn probe_encoder(ffmpeg: &Path, encoder: Encoder, fps: u32) -> Result<(), String> {
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error"]
        .map(String::from)
        .to_vec();
    args.extend(capture_args(encoder, fps));
    args.extend(["-frames:v", "10", "-f", "null", "-"].map(String::from));
    let mut child = Command::new(ffmpeg)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = io::Read::read_to_string(&mut pipe, &mut stderr);
            }
            let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            return Err(format!("test capture failed ({}) {}", status, first.trim()));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("test capture timed out".to_string());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// How a recording ended.
#[derive(Debug)]
pub enum Stopped {
    /// ffmpeg finished the file after `q`.
    Finished(ExitStatus),
    /// ffmpeg had already exited before it was asked to stop (see its log).
    ExitedEarly(ExitStatus),
    /// ffmpeg did not exit within the timeout and was killed (the file is still playable).
    Killed,
}

/// A running ffmpeg (or any process that stops when it reads `q` on stdin).
pub struct Recording {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Recording {
    /// Spawns `cmd` with stdin piped and stderr written to `log`. On Windows the process
    /// gets its own process group, so a Ctrl+C in the console reaches only Squire (which
    /// then stops the recording itself), and it is killed if Squire exits without stopping
    /// it.
    pub fn spawn(mut cmd: Command, log: &Path) -> io::Result<Self> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(File::create(log)?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = cmd.spawn()?;
        kill_on_exit(&child);
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    /// Sends `q`, waits up to `timeout` for the process to exit, then kills it.
    pub fn stop(mut self, timeout: Duration) -> io::Result<Stopped> {
        if let Some(status) = self.child.try_wait()? {
            return Ok(Stopped::ExitedEarly(status));
        }
        if let Some(mut stdin) = self.stdin.take() {
            // ffmpeg may exit between try_wait and the write; a broken pipe is fine then.
            let _ = stdin.write_all(b"q");
            let _ = stdin.flush();
        }
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(Stopped::Finished(status));
            }
            if Instant::now() >= deadline {
                self.child.kill()?;
                self.child.wait()?;
                return Ok(Stopped::Killed);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Puts the process in a job that is killed when Squire exits, so a crash or a closed
/// console never leaves ffmpeg recording.
#[cfg(windows)]
fn kill_on_exit(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // The job handle is never closed: Windows closes it, and kills the job, on exit.
    static JOB: OnceLock<Option<isize>> = OnceLock::new();
    let job = JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(None, None).ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .ok()?;
        Some(job.0 as isize)
    });
    if let Some(job) = job {
        let process = HANDLE(child.as_raw_handle());
        if let Err(e) = unsafe { AssignProcessToJobObject(HANDLE(*job as _), process) } {
            eprintln!(
                "[Video] ffmpeg will not be stopped if Squire crashes: {}",
                e
            );
        }
    }
}

#[cfg(not(windows))]
fn kill_on_exit(_child: &Child) {}

struct Active {
    recording: Recording,
    video: PathBuf,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

#[derive(serde::Serialize)]
struct Sidecar<'a> {
    video: &'a str,
    /// Wall-clock time just before ffmpeg was spawned. The first video frame follows
    /// ffmpeg's startup (typically a few hundred ms); `tools/align_video.py` measures the
    /// exact offset.
    ffmpeg_started_unix_ms: u64,
    /// Capture time of the frame on which the match was detected (the CSV's first rows).
    match_start_unix_ms: u64,
    encoder: &'a str,
    fps: u32,
    command: Vec<String>,
}

/// Starts recording a match: `<dir>/<stem>.mkv`, `<stem>.video.json` and
/// `<stem>.ffmpeg.log`. Stops a recording that is still running first.
pub fn start(opts: &VideoOptions, stem: &str, match_start_unix_ms: u64) -> io::Result<PathBuf> {
    stop_active();
    std::fs::create_dir_all(&opts.dir)?;
    let video = opts.dir.join(format!("{}.mkv", stem));
    let args = record_args(opts.encoder, opts.fps, &video);
    let mut cmd = Command::new(&opts.ffmpeg);
    cmd.args(&args);

    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let recording = Recording::spawn(cmd, &opts.dir.join(format!("{}.ffmpeg.log", stem)))?;

    let video_name = format!("{}.mkv", stem);
    let sidecar = Sidecar {
        video: &video_name,
        ffmpeg_started_unix_ms: started,
        match_start_unix_ms,
        encoder: opts.encoder.codec(),
        fps: opts.fps,
        command: std::iter::once(opts.ffmpeg.as_os_str())
            .chain(args.iter().map(OsString::as_os_str))
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
    };
    let sidecar_path = opts.dir.join(format!("{}.video.json", stem));
    if let Err(e) = serde_json::to_string_pretty(&sidecar)
        .map_err(io::Error::from)
        .and_then(|json| std::fs::write(&sidecar_path, json))
    {
        eprintln!("[Video] Could not write {}: {}", sidecar_path.display(), e);
    }

    *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Active {
        recording,
        video: video.clone(),
    });
    Ok(video)
}

/// Stops the running recording, if any, and reports how it ended. Returns the video path.
pub fn stop_active() -> Option<PathBuf> {
    let active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    let path = active.video.display().to_string();
    match active.recording.stop(STOP_TIMEOUT) {
        Ok(Stopped::Finished(status)) if status.success() => {
            println!("[Video] Saved {}", path)
        }
        Ok(Stopped::Finished(status)) => {
            eprintln!("[Video] ffmpeg exited with {} for {}", status, path)
        }
        Ok(Stopped::ExitedEarly(status)) => eprintln!(
            "[Video] ffmpeg had stopped early ({}); see the .ffmpeg.log next to {}",
            status, path
        ),
        Ok(Stopped::Killed) => eprintln!(
            "[Video] ffmpeg did not stop within {} s and was killed; {} should still play",
            STOP_TIMEOUT.as_secs(),
            path
        ),
        Err(e) => eprintln!("[Video] Could not stop ffmpeg for {}: {}", path, e),
    }
    Some(active.video)
}

/// Stops the running recording when dropped, so a capture loop that returns early with an
/// error does not leave ffmpeg recording.
pub struct StopOnDrop;

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        stop_active();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_args_capture_output_0_and_write_matroska() {
        let video = Path::new(r"C:\videos\telemetry_20261031_120000.mkv");
        let args: Vec<String> = record_args(Encoder::Nvenc, 30, video)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        let pos = |s: &str| args.iter().position(|a| a == s).unwrap();

        assert_eq!(args[pos("-f") + 1], "lavfi");
        assert_eq!(args[pos("-i") + 1], "ddagrab=output_idx=0:framerate=30");
        assert_eq!(args[pos("-c:v") + 1], "h264_nvenc");
        assert_eq!(args[pos("-g") + 1], "60");
        assert_eq!(
            args[args.len() - 3..],
            ["-f", "matroska", video.to_str().unwrap()]
        );
        assert!(args.contains(&"-y".to_string()));
    }

    #[test]
    fn every_encoder_gets_its_codec_and_gpu_frame_handling() {
        for encoder in Encoder::ALL {
            let args = capture_args(encoder, 25);
            let codec = &args[args.iter().position(|a| a == "-c:v").unwrap() + 1];
            assert_eq!(codec, encoder.codec());
            assert_eq!(Encoder::parse(encoder.name()), Some(encoder));
            assert!(args.contains(&"ddagrab=output_idx=0:framerate=25".to_string()));

            let filter = args
                .iter()
                .position(|a| a == "-vf")
                .map(|i| args[i + 1].as_str());
            match encoder {
                Encoder::Nvenc | Encoder::Amf => assert_eq!(filter, None),
                Encoder::Qsv => assert!(filter.unwrap().starts_with("hwmap=derive_device=qsv")),
                Encoder::X264 => assert!(filter.unwrap().starts_with("hwdownload")),
            }
        }
        assert_eq!(Encoder::parse("h265"), None);
    }

    #[test]
    fn version_and_capability_checks() {
        assert_eq!(
            parse_version("ffmpeg version 4.0 Copyright (c) 2000-2018 the FFmpeg developers"),
            Some((4, 0))
        );
        assert_eq!(
            parse_version("ffmpeg version 7.1-full_build-www.gyan.dev Copyright"),
            Some((7, 1))
        );
        assert_eq!(
            parse_version("ffmpeg version N-112233-gabcdef Copyright"),
            None
        );

        let old = check_capabilities(
            "ffmpeg version 4.0 Copyright",
            " ... gdigrab-free list\n T.. hflip    V->V  Horizontally flip the input video.",
        )
        .unwrap_err();
        assert!(old.contains("ffmpeg 4.0") && old.contains("6.0"), "{}", old);
        assert!(check_capabilities(
            "ffmpeg version N-1-g1",
            " ... ddagrab           |->V       Grab Windows Desktop images using DXGI.",
        )
        .is_ok());

        let encoders = " V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)";
        assert!(lists_encoder(encoders, "h264_nvenc"));
        assert!(!lists_encoder(encoders, "h264_amf"));
    }

    #[test]
    fn ffmpeg_path_prefers_the_flag() {
        assert_eq!(
            ffmpeg_path(Some(r"D:\ff\ffmpeg.exe")),
            PathBuf::from(r"D:\ff\ffmpeg.exe")
        );
    }

    /// Runs this test binary as a stand-in for ffmpeg, in the given mode (see
    /// `stand_in_ffmpeg`).
    fn stand_in(mode: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "capture::video::tests::stand_in_ffmpeg",
            "--exact",
            "--nocapture",
        ])
        .env("SQUIRE_STAND_IN", mode);
        cmd
    }

    /// Not a test: the stand-in process for the lifecycle tests. "obey" exits once it reads
    /// `q`, "ignore" never exits by itself, "crash" exits at once with status 3.
    #[test]
    fn stand_in_ffmpeg() {
        let Ok(mode) = std::env::var("SQUIRE_STAND_IN") else {
            return;
        };
        match mode.as_str() {
            "obey" => {
                let mut byte = [0u8];
                while io::Read::read(&mut io::stdin(), &mut byte).unwrap_or(0) == 1 {
                    if byte[0] == b'q' {
                        std::process::exit(0);
                    }
                }
                std::process::exit(1); // stdin closed without q
            }
            "ignore" => loop {
                std::thread::sleep(Duration::from_secs(60));
            },
            _ => std::process::exit(3),
        }
    }

    fn log_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("squire_video_{}_{}.log", name, std::process::id()))
    }

    #[test]
    fn stop_sends_q_and_waits_for_exit() {
        let rec = Recording::spawn(stand_in("obey"), &log_path("obey")).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        match rec.stop(Duration::from_secs(20)).unwrap() {
            Stopped::Finished(status) => assert!(status.success()),
            other => panic!("expected a clean stop, got {:?}", other),
        }
    }

    #[test]
    fn stop_kills_a_process_that_ignores_q() {
        let rec = Recording::spawn(stand_in("ignore"), &log_path("ignore")).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let start = Instant::now();
        assert!(matches!(
            rec.stop(Duration::from_millis(500)).unwrap(),
            Stopped::Killed
        ));
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn stop_reports_a_process_that_exited_early() {
        let mut rec = Recording::spawn(stand_in("crash"), &log_path("crash")).unwrap();
        let start = Instant::now();
        while rec.child.try_wait().unwrap().is_none() && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(50));
        }
        match rec.stop(Duration::from_secs(1)).unwrap() {
            Stopped::ExitedEarly(status) => assert_eq!(status.code(), Some(3)),
            other => panic!("expected an early exit, got {:?}", other),
        }
    }
}
