//! Sub4 main-UI legacy adapter; endpoint behavior preserved.
use super::*;

#[derive(Deserialize)]
pub(super) struct LegacySqliteInput { path:String, #[serde(default)] confirm:bool }
pub(super) fn legacy_sqlite_path(path:&str)->Result<std::path::PathBuf,Fault>{
    let path=path.trim();
    if path.is_empty()||path.len()>4096{return Err(bad("legacy SQLite path required (up to 4096 characters)"));}
    let path=std::path::PathBuf::from(path);
    if !path.is_absolute(){return Err(bad("legacy SQLite source path must be absolute"));}
    Ok(path)
}
pub(super) async fn legacy_sqlite_preview(State(engine):State<Arc<Engine>>,
    Json(input):Json<LegacySqliteInput>)->Result<Json<crate::legacy_import::LegacyPreview>,Fault>{
    let path=legacy_sqlite_path(&input.path)?;
    Dominion::from_engine(engine).preview_legacy_sqlite(path).await
        .map(Json).map_err(bad)
}
pub(super) async fn legacy_sqlite_import(State(engine):State<Arc<Engine>>,
    Json(input):Json<LegacySqliteInput>)->Result<Json<crate::legacy_import::LegacyImportResult>,Fault>{
    if !input.confirm{return Err(bad("explicit confirm:true required before copying legacy records"));}
    let path=legacy_sqlite_path(&input.path)?;
    Dominion::from_engine(engine).import_legacy_sqlite(path).await
        .map(Json).map_err(bad)
}

pub(super) async fn legacy_sqlite_archive(State(engine):State<Arc<Engine>>,
    Json(input):Json<LegacySqliteInput>)->Result<Json<crate::legacy_archive::ArchiveStats>,Fault>{
    if !input.confirm{return Err(bad("explicit confirm:true required before archiving historical records"));}
    let path=legacy_sqlite_path(&input.path)?;
    Dominion::from_engine(engine).archive_legacy_sqlite(path).await.map(Json).map_err(bad)
}
#[derive(Deserialize)]
pub(super) struct LegacyHistoryQuery {
    kind:String,
    #[serde(default)] offset:usize,
    limit:Option<usize>,
}
pub(super) async fn legacy_archive_list(State(engine):State<Arc<Engine>>,
    Query(input):Query<LegacyHistoryQuery>)->Result<Json<crate::legacy_archive::ArchivePage>,Fault>{
    Dominion::from_engine(engine).list_legacy_history(
        input.kind,input.offset,input.limit.unwrap_or(30)
    ).await.map(Json).map_err(bad)
}
#[derive(Deserialize)]
pub(super) struct LegacyHistoryItemQuery {kind:String,id:String}
pub(super) async fn legacy_archive_item(State(engine):State<Arc<Engine>>,
    Query(input):Query<LegacyHistoryItemQuery>)->Result<Json<Value>,Fault>{
    Dominion::from_engine(engine).legacy_history_item(input.kind,input.id).await
        .map_err(bad)?.map(Json).ok_or_else(||missing("legacy archive record not found"))
}
#[derive(Deserialize)]
pub(super) struct LegacyPromoteInput{
    id:String,scope:String,
    #[serde(default)]confirm:bool
}
pub(super) async fn legacy_archive_promote_memory(State(engine):State<Arc<Engine>>,
    Json(input):Json<LegacyPromoteInput>)->Result<Json<crate::knowledge::MemoryEntry>,Fault>{
    if !input.confirm{return Err(bad("explicit confirm:true required to promote archived memory"));}
    Dominion::from_engine(engine).promote_legacy_memory(input.id,input.scope).await
        .map(Json).map_err(bad)
}

