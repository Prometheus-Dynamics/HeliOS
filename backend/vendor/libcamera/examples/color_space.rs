use libcamera::{
    camera_manager::CameraManager,
    color_space::{ColorSpace, Primaries, Range, TransferFunction, YcbcrEncoding},
    logging::LoggingLevel,
    stream::StreamRole,
};

fn main() {
    let mgr = CameraManager::new().expect("camera manager");
    mgr.log_set_level("Camera", LoggingLevel::Error);
    let cameras = mgr.cameras();
    let cam = cameras.iter().next().expect("no cameras found");

    let mut cfgs = cam
        .generate_configuration(&[StreamRole::ViewFinder])
        .expect("generate configuration");
    {
        let mut cfg = cfgs.get_mut(0).expect("cfg");

        // Inspect existing color space if present
        println!("Original color space: {:?}", cfg.get_color_space());

        // Set Rec709 with limited range as an example
        let cs = ColorSpace::new(
            Primaries::Rec709,
            TransferFunction::Rec709,
            YcbcrEncoding::Rec709,
            Range::Limited,
        );
        cfg.set_color_space(Some(cs));
        println!("Requested color space: {:?}", cfg.get_color_space());
    }

    // Apply configuration to have libcamera validate it
    let status = cfgs.validate();
    println!("Validation status: {:?}", status);
    println!(
        "Validated color space: {:?}",
        cfgs.get(0).expect("cfg").get_color_space()
    );
}
