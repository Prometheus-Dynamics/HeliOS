//! Demonstrates orientation helpers wrapping libcamera::orientationFromRotation and transform composition.
use libcamera::{
    camera::Orientation,
    transform::{apply_transform_to_orientation, orientation_from_rotation, Transform},
};

fn main() {
    // Map a rotation to an EXIF orientation.
    for angle in [0, 90, 180, 270] {
        let ori = orientation_from_rotation(angle).expect("angle should be valid");
        println!("rotation {} => orientation {:?}", angle, ori);
    }

    // Combine orientations via transforms.
    let from = Orientation::Rotate0;
    let to = Orientation::Rotate90;
    let t = Transform::between_orientations(from, to);
    println!("Transform from {:?} to {:?}: {}", from, to, t);

    // Apply an arbitrary transform to an orientation.
    let rotated = apply_transform_to_orientation(Orientation::Rotate90, Transform::from_rotation(180, false).unwrap());
    println!("Rotate90 with extra 180deg => {:?}", rotated);
}
