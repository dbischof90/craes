use diesel::sql_types::Timestamp;

#[derive(Queryable)]
pub struct AssetInfo {
    pub id: i32,
    pub name: String,
    pub version_id: i8,
    pub valid_from: Timestamp,
    pub valid_to: Timestamp,
}
