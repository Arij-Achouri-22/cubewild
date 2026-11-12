//! Coordinated world checkpoints. A durable journal is published before any live
//! file changes; opening the world finishes an interrupted commit before loading it.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::world::ChunkData;

const WRITING: &str = ".checkpoint-writing";
const PENDING: &str = ".checkpoint-pending";
const BACKUP: &str = ".checkpoint-backup";

/// New state files are optional for older worlds; other read failures must surface.
pub fn read_optional(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

enum Contents {
    Text(String),
    Chunk(Arc<ChunkData>),
}

/// Immutable player/container data and shared chunk snapshots from one simulation
/// tick. Only encoding happens on the game thread; all disk work can run elsewhere.
pub struct Snapshot {
    root: PathBuf,
    files: Vec<(PathBuf, Contents)>,
}

impl Snapshot {
    pub fn new(root: PathBuf) -> Self {
        Self { root, files: Vec::new() }
    }

    pub fn text(&mut self, path: impl Into<PathBuf>, text: String) {
        self.files.push((path.into(), Contents::Text(text)));
    }

    pub fn chunk(&mut self, path: PathBuf, data: Arc<ChunkData>) {
        self.files.push((path, Contents::Chunk(data)));
    }

    pub fn commit(self) -> io::Result<()> {
        let checkpoint = Checkpoint::begin(&self.root)?;
        for (relative, contents) in self.files {
            if relative.as_os_str().is_empty() || relative.components().any(|p| !matches!(p, Component::Normal(_))) {
                return Err(invalid("invalid snapshot path"));
            }
            let path = checkpoint.dir().join(relative);
            fs::create_dir_all(path.parent().unwrap())?;
            match &contents {
                Contents::Text(text) => fs::write(path, text.as_bytes())?,
                Contents::Chunk(data) => fs::write(path, crate::registry::encode(data))?,
            }
        }
        checkpoint.commit()
    }
}

pub struct Background {
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl Background {
    pub fn start(snapshot: Snapshot) -> io::Result<Self> {
        let worker = std::thread::Builder::new().name("world-save".into()).spawn(move || snapshot.commit())?;
        Ok(Self { worker: Some(worker) })
    }

    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().unwrap().is_finished()
    }

    pub fn wait(mut self) -> io::Result<()> {
        self.worker.take().unwrap().join().unwrap_or_else(|_| Err(io::Error::other("the world save worker stopped unexpectedly")))
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        // A world must never remove or reuse its directory while its writer runs.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub struct Checkpoint {
    root: PathBuf,
    staging: PathBuf,
}

impl Checkpoint {
    pub fn begin(root: &Path) -> io::Result<Self> {
        recover(root)?;
        let staging = root.join(WRITING);
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        fs::create_dir_all(staging.join("new"))?;
        Ok(Self { root: root.to_path_buf(), staging })
    }

    /// All save producers write here, including the current dimension's chunks.
    pub fn dir(&self) -> PathBuf {
        self.staging.join("new")
    }

    fn prepare(&self) -> io::Result<()> {
        let mut manifest = String::new();
        let mut created = String::new();
        for relative in files_in(&self.dir())? {
            let source = self.dir().join(&relative);
            let bytes = fs::read(&source)?;
            validate_chunk(&relative, &bytes)?;
            File::options().write(true).open(&source)?.sync_all()?;
            let name = relative.to_str().ok_or_else(|| invalid("invalid checkpoint filename"))?;
            manifest += &format!("{:016x} {} {}\n", checksum(&bytes), bytes.len(), name);
            let original = self.root.join(&relative);
            match fs::read(&original) {
                Ok(old) => {
                    let backup = self.staging.join("old").join(&relative);
                    fs::create_dir_all(backup.parent().unwrap())?;
                    write_synced(&backup, &old)?;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => created += &format!("{name}\n"),
                Err(e) => return Err(e),
            }
        }
        manifest += &format!("\nend {:016x}\n", checksum(manifest.as_bytes()));
        write_synced(&self.staging.join("manifest.txt"), manifest.as_bytes())?;
        write_synced(&self.staging.join("created.txt"), created.as_bytes())?;
        sync_tree(&self.staging)
    }

    pub fn commit(self) -> io::Result<()> {
        self.prepare()?;
        fs::rename(&self.staging, self.root.join(PENDING))?;
        sync_dir(&self.root)?;
        recover(&self.root)
    }
}

/// Ignores unpublished writes. A published journal is complete and can be replayed
/// repeatedly, even if the process stops while replacing one of the live files.
pub fn recover(root: &Path) -> io::Result<()> {
    let pending = root.join(PENDING);
    if !pending.exists() {
        return Ok(());
    }
    let text = fs::read_to_string(pending.join("manifest.txt"))?;
    let (text, footer) = text.rsplit_once("\nend ").ok_or_else(|| invalid("incomplete checkpoint manifest"))?;
    let hash = footer.strip_suffix('\n').and_then(|v| u64::from_str_radix(v, 16).ok()).ok_or_else(|| invalid("invalid checkpoint manifest"))?;
    if checksum(text.as_bytes()) != hash {
        return Err(invalid("damaged checkpoint manifest"));
    }
    let mut entries = Vec::new();
    // Verify the whole checkpoint before changing any live file.
    for line in text.lines() {
        let mut parts = line.splitn(3, ' ');
        let hash = parts.next().and_then(|v| u64::from_str_radix(v, 16).ok()).ok_or_else(|| invalid("invalid checkpoint checksum"))?;
        let len = parts.next().and_then(|v| v.parse::<usize>().ok()).ok_or_else(|| invalid("invalid checkpoint length"))?;
        let relative = PathBuf::from(parts.next().ok_or_else(|| invalid("missing checkpoint filename"))?);
        if relative.as_os_str().is_empty() || relative.components().any(|p| !matches!(p, Component::Normal(_))) {
            return Err(invalid("invalid checkpoint path"));
        }
        let bytes = fs::read(pending.join("new").join(&relative))?;
        if bytes.len() != len || checksum(&bytes) != hash {
            return Err(invalid(&format!("damaged checkpoint file: {}", relative.display())));
        }
        validate_chunk(&relative, &bytes)?;
        entries.push(relative);
    }
    if entries.is_empty() {
        return Err(invalid("empty checkpoint manifest"));
    }
    for relative in entries {
        let target = root.join(&relative);
        fs::create_dir_all(target.parent().unwrap())?;
        atomic_write(&target, &fs::read(pending.join("new").join(relative))?)?;
    }
    let backup = root.join(BACKUP);
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    fs::rename(pending, backup)?;
    sync_dir(root)
}

/// Readers either see the previous whole file or the new one, never a truncated one.
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_file_name(format!("{}.checkpoint-tmp", path.file_name().unwrap().to_string_lossy()));
    write_synced(&tmp, bytes)?;
    fs::rename(tmp, path)?;
    sync_dir(path.parent().unwrap())
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| (hash ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate_chunk(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with("c.") && name.ends_with(".bin") {
        if bytes.starts_with(crate::registry::MAGIC) {crate::registry::decode(bytes)?;}
        else if bytes.len()!=crate::world::VOL && bytes.len()!=crate::world::GVOL {
            return Err(invalid(&format!("damaged saved chunk: {} ({} bytes)",path.display(),bytes.len())));
        }
    }
    Ok(())
}

fn files_in(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            paths.extend(files_in(&entry.path())?.into_iter().map(|p| PathBuf::from(entry.file_name()).join(p)));
        } else {
            paths.push(PathBuf::from(entry.file_name()));
        }
    }
    paths.sort();
    Ok(paths)
}

fn sync_tree(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_tree(&entry.path())?;
        }
    }
    sync_dir(dir)
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    // File data is synced above; opening directories for sync is Unix-specific.
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::world::{PLANKS, STONE, VOL};

    pub(crate) struct TestDir(pub PathBuf);

    impl TestDir {
        pub(crate) fn new(label: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("cubewild-{label}-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn stage(root: &Path) -> Checkpoint {
        let checkpoint = Checkpoint::begin(root).unwrap();
        fs::create_dir_all(checkpoint.dir().join("deep")).unwrap();
        fs::write(checkpoint.dir().join("player.txt"), "new inventory\n").unwrap();
        fs::write(checkpoint.dir().join("c.0.0.bin"), vec![PLANKS as u8; VOL]).unwrap();
        fs::write(checkpoint.dir().join("deep/chests.txt"), "new chest contents\n").unwrap();
        checkpoint
    }

    #[test]
    fn dropping_the_save_handle_finishes_the_write_before_a_world_can_be_removed() {
        let root = TestDir::new("background-drop");
        let mut snapshot = Snapshot::new(root.0.clone());
        snapshot.text("player.txt", "completed snapshot\n".into());
        let writer = Background::start(snapshot).unwrap();
        drop(writer);
        assert_eq!(fs::read_to_string(root.0.join("player.txt")).unwrap(), "completed snapshot\n");
        assert!(!root.0.join(PENDING).exists());
    }

    #[test]
    fn background_save_errors_are_returned_to_the_game() {
        let root = TestDir::new("background-error");
        let blocked = root.0.join("not-a-directory");
        fs::write(&blocked, "keep this file").unwrap();
        let mut snapshot = Snapshot::new(blocked.clone());
        snapshot.text("player.txt", "new snapshot\n".into());
        assert!(Background::start(snapshot).unwrap().wait().is_err());
        assert_eq!(fs::read_to_string(blocked).unwrap(), "keep this file");
    }

    #[test]
    fn checkpoint_keeps_the_previous_files_and_commits_both_dimensions() {
        let root = TestDir::new("checkpoint");
        fs::write(root.0.join("player.txt"), "old inventory\n").unwrap();
        fs::write(root.0.join("c.0.0.bin"), vec![STONE as u8; VOL]).unwrap();
        stage(&root.0).commit().unwrap();
        assert_eq!(fs::read_to_string(root.0.join("player.txt")).unwrap(), "new inventory\n");
        assert_eq!(fs::read(root.0.join("c.0.0.bin")).unwrap(), vec![PLANKS as u8; VOL]);
        assert_eq!(fs::read_to_string(root.0.join("deep/chests.txt")).unwrap(), "new chest contents\n");
        assert_eq!(fs::read_to_string(root.0.join(BACKUP).join("old/player.txt")).unwrap(), "old inventory\n");
        assert_eq!(fs::read(root.0.join(BACKUP).join("old/c.0.0.bin")).unwrap(), vec![STONE as u8; VOL]);
        assert!(fs::read_to_string(root.0.join(BACKUP).join("created.txt")).unwrap().contains("deep/chests.txt"));
        assert!(!root.0.join(PENDING).exists());
    }

    #[test]
    fn interrupted_commit_finishes_before_any_world_state_is_loaded() {
        let root = TestDir::new("recover");
        fs::write(root.0.join("player.txt"), "old inventory\n").unwrap();
        fs::write(root.0.join("c.0.0.bin"), vec![STONE as u8; VOL]).unwrap();
        let checkpoint = stage(&root.0);
        checkpoint.prepare().unwrap();
        fs::rename(&checkpoint.staging, root.0.join(PENDING)).unwrap();
        // The process stopped after the terrain was replaced, before the player.
        atomic_write(&root.0.join("c.0.0.bin"), &vec![PLANKS as u8; VOL]).unwrap();
        recover(&root.0).unwrap();
        assert_eq!(fs::read_to_string(root.0.join("player.txt")).unwrap(), "new inventory\n");
        assert_eq!(fs::read(root.0.join("c.0.0.bin")).unwrap(), vec![PLANKS as u8; VOL]);
        recover(&root.0).unwrap();
    }

    #[test]
    fn incomplete_or_damaged_staging_never_replaces_live_files() {
        let root = TestDir::new("incomplete");
        fs::write(root.0.join("player.txt"), "old inventory\n").unwrap();
        let checkpoint = stage(&root.0);
        recover(&root.0).unwrap(); // unpublished work is ignored
        assert_eq!(fs::read_to_string(root.0.join("player.txt")).unwrap(), "old inventory\n");
        checkpoint.prepare().unwrap();
        fs::rename(&checkpoint.staging, root.0.join(PENDING)).unwrap();
        fs::write(root.0.join(PENDING).join("new/c.0.0.bin"), [STONE as u8]).unwrap();
        assert!(recover(&root.0).is_err());
        assert_eq!(fs::read_to_string(root.0.join("player.txt")).unwrap(), "old inventory\n");
        assert!(!root.0.join("deep/chests.txt").exists());
        assert!(root.0.join(PENDING).exists());
    }

    #[test]
    fn checkpoint_failure_is_returned_without_touching_the_world() {
        let root = TestDir::new("save-failure");
        fs::write(root.0.join("player.txt"), "old inventory\n").unwrap();
        let checkpoint = stage(&root.0);
        fs::create_dir(checkpoint.staging.join("manifest.txt")).unwrap();
        assert!(checkpoint.commit().is_err());
        assert_eq!(fs::read_to_string(root.0.join("player.txt")).unwrap(), "old inventory\n");
    }
}
