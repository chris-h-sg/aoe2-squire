use rts_analyzer::capture::bgra_to_rgb_image;
use windows::core::{Interface, Result};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_OUTDUPL_FRAME_INFO,
};

#[test]
fn test_live_screen_capture() -> Result<()> {
    // NOTE: This test will fail in headless CI environments because it requires a physical GPU
    // and an active Windows Desktop session to duplicate the screen.
    // If a CI pipeline is added in the future, this test should be flagged with #[ignore].

    let mut device = None;
    let mut context = None;
    let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    
    let device: ID3D11Device = device.unwrap();
    let context: ID3D11DeviceContext = context.unwrap();

    let dxgi_device: IDXGIDevice = device.cast()?;
    let adapter = unsafe { dxgi_device.GetAdapter()? };
    let output = unsafe { adapter.EnumOutputs(0)? }; 
    let output1: IDXGIOutput1 = output.cast()?;
    let duplication: IDXGIOutputDuplication = unsafe { output1.DuplicateOutput(&device)? };

    let output_desc = unsafe { output.GetDesc()? };
    let width = (output_desc.DesktopCoordinates.right - output_desc.DesktopCoordinates.left) as u32;
    let height = (output_desc.DesktopCoordinates.bottom - output_desc.DesktopCoordinates.top) as u32;

    assert!(width > 0 && height > 0, "Width and height must be greater than 0");

    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    
    let mut staging_texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut staging_texture))? };
    let staging_texture: ID3D11Texture2D = staging_texture.unwrap();

    let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
    let mut desktop_resource: Option<IDXGIResource> = None;
    
    // Retry a few times; Desktop Duplication only yields frames when the screen actually updates
    let mut attempts = 0;
    let mut success = false;
    
    while attempts < 10 {
        let res = unsafe { duplication.AcquireNextFrame(100, &mut frame_info, &mut desktop_resource) };
        if res.is_ok() {
            success = true;
            break;
        }
        attempts += 1;
    }
    
    assert!(success, "Failed to acquire a frame after 10 attempts. Make sure the screen is updating.");

    let desktop_resource = desktop_resource.unwrap();
    let desktop_texture: ID3D11Texture2D = desktop_resource.cast()?;

    unsafe {
        context.CopyResource(&staging_texture, &desktop_texture);
    }

    let mut mapped: D3D11_MAPPED_SUBRESOURCE = Default::default();
    unsafe {
        let map_res = context.Map(&staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped));
        assert!(map_res.is_ok(), "Failed to map staging texture");
        
        let ptr = mapped.pData as *const u8;
        let pitch = mapped.RowPitch as usize;
        
        assert!(!ptr.is_null());
        assert!(pitch >= (width as usize) * 4);
        
        let data_slice = std::slice::from_raw_parts(ptr, (height as usize) * pitch);
        let rgb_image = bgra_to_rgb_image(data_slice, width, height, pitch);
        
        assert_eq!(rgb_image.width(), width);
        assert_eq!(rgb_image.height(), height);

        context.Unmap(&staging_texture, 0);
    }

    unsafe {
        let _ = duplication.ReleaseFrame();
    }
    
    Ok(())
}
