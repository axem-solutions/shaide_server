use std::{
    cell::Cell,
    env,
    path::PathBuf,
    sync::{LazyLock, Mutex},
};

static SHAIDE_ROOT: LazyLock<Mutex<Cell<PathBuf>>> = LazyLock::new(|| {
    Mutex::new(Cell::new(match env::var("shaide_ROOT") {
        Ok(x) => PathBuf::from(x),
        Err(_) => home::home_dir().unwrap().join(".config/axem/shaide"),
    }))
});

pub fn shaide_root() -> PathBuf {
    let mut cell = SHAIDE_ROOT.lock().unwrap();
    cell.get_mut().clone()
}

fn shaide_db_root() -> PathBuf {
    shaide_root().join("db")
}

pub fn get_db_file() -> PathBuf {
    shaide_db_root().join("shaide-server.sqlite")
}
