#[allow(dead_code)]
pub fn reset_lifecycle() {
    let pddb = pddb::Pddb::new();
    pddb.delete_dict(crate::config::DC34_DICT, None).ok();
}
