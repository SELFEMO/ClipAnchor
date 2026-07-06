use std::{env, fs, path::PathBuf};

#[derive(Clone, Debug)]
pub struct DataPaths {
    pub root: PathBuf,
    pub data: PathBuf,
    pub database: PathBuf,
    pub settings: PathBuf,
    pub resources: PathBuf,
    pub exports: PathBuf,
    pub logs: PathBuf,
}

pub fn resolve() -> Result<DataPaths, String> {
    let exe = env::current_exe().map_err(|error| error.to_string())?;
    let root = exe.parent().ok_or_else(|| "Cannot resolve executable directory".to_string())?.to_path_buf();
    let data = root.join("data");

    // 所有运行时数据都锚定到可执行文件同级 data 目录，避免平台默认缓存路径破坏便携性。
    // All runtime data is anchored beside the executable so platform cache folders cannot break portability.
    Ok(DataPaths {
        root,
        database: data.join("clipanchor.db"),
        settings: data.join("settings.json"),
        resources: data.join("resources"),
        exports: data.join("exports"),
        logs: data.join("logs"),
        data,
    })
}

pub fn ensure(paths: &DataPaths) -> Result<(), String> {
    for path in [&paths.data, &paths.resources, &paths.exports, &paths.logs] {
        fs::create_dir_all(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}
