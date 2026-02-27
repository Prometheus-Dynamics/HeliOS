//! Demonstrates ControlList helpers: contains/len/merge and string array ControlValue support.
use libcamera::{
    control::{ControlList, MergePolicy},
    control_value::ControlValue,
    properties::PropertyId,
};

fn main() {
    // Build a control list with a string array value to exercise ControlValue string handling.
    let mut base = ControlList::new();
    let tags = vec!["alpha".to_string(), "beta".to_string()];
    base.set_raw(PropertyId::Model as u32, ControlValue::from(tags))
        .unwrap();

    println!(
        "Base list len={} contains Model? {}",
        base.len(),
        base.contains(PropertyId::Model as u32)
    );

    // Merge another list with a different value for the same id.
    let mut override_list = ControlList::new();
    override_list
        .set_raw(PropertyId::Model as u32, ControlValue::from("override".to_string()))
        .unwrap();

    base.merge(&override_list, MergePolicy::OverwriteExisting);
    if let Ok(ControlValue::String(v)) = base.get_raw(PropertyId::Model as u32) {
        println!("After merge (overwrite), model entries: {:?}", v.into_vec());
    }

    // Clear/reset.
    base.clear();
    println!("After clear, is_empty? {}", base.is_empty());
}
