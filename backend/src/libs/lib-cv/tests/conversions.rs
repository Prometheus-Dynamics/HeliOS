use lib_cv::{Rotation2, Rotation3, Translation2, Translation3};

#[test]
fn translation_from_tuple_and_array() {
    let t1: Translation3 = (1.0, 2.0, 3.0).into();
    let t2: Translation3 = [4.0, 5.0, 6.0].into();
    assert_eq!(t1.x, 1.0);
    assert_eq!(t1.y, 2.0);
    assert_eq!(t1.z, 3.0);
    assert_eq!(t2, Translation3 { x: 4.0, y: 5.0, z: 6.0 });

    let t3: Translation2 = (7.0, 8.0).into();
    let t4: Translation2 = [9.0, 10.0].into();
    assert_eq!(t3, Translation2 { x: 7.0, y: 8.0 });
    assert_eq!(t4, Translation2 { x: 9.0, y: 10.0 });
}

#[test]
fn rotation_from_tuple_and_array() {
    let r1: Rotation3 = (0.1, 0.2, 0.3).into();
    let r2: Rotation3 = [0.4, 0.5, 0.6].into();
    assert_eq!(r1.roll, 0.1);
    assert_eq!(r1.pitch, 0.2);
    assert_eq!(r1.yaw, 0.3);
    assert_eq!(r2, Rotation3 { roll: 0.4, pitch: 0.5, yaw: 0.6 });

    let r3: Rotation2 = 0.7f64.into();
    assert_eq!(r3, Rotation2 { yaw: 0.7 });
}
