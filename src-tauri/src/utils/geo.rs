use crate::utils::dirs::mihomo_work_dir;
use std::path::Path;

pub const GEO_FILES: [&str; 4] = ["country.mmdb", "geoip.dat", "geosite.dat", "ASN.mmdb"];

pub async fn remove_unused_metadb(dir: &Path) {
    if dir.join("country.mmdb").exists() {
        let _ = tokio::fs::remove_file(dir.join("geoip.metadb")).await;
    }
}

async fn link_geo_file(file: &str, target_dir: &Path) {
    let source = mihomo_work_dir().join(file);
    let target = target_dir.join(file);
    if !source.exists() {
        return;
    }
    if target == source {
        return;
    }

    let source_meta = tokio::fs::metadata(&source).await.ok();
    let target_meta = tokio::fs::metadata(&target).await.ok();

    if let (Some(sm), Some(tm)) = (source_meta, target_meta) {
        let s_mod = sm.modified().ok();
        let t_mod = tm.modified().ok();
        if let (Some(sm_t), Some(tm_t)) = (s_mod, t_mod) {
            if tm_t > sm_t {
                let _ = tokio::fs::rename(&target, &source).await;
            } else {
                let _ = tokio::fs::remove_file(&target).await;
            }
        }
    }

    if !target.exists() && std::fs::hard_link(&source, &target).is_err() {
        let _ = tokio::fs::copy(&source, &target).await;
    }
}

pub async fn link_geo_files(target_dir: &Path) {
    remove_unused_metadb(target_dir).await;
    for file in GEO_FILES {
        link_geo_file(file, target_dir).await;
    }
}
