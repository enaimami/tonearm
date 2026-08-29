//! Export arşivine erişim soyutlaması.
//!
//! Dosya sistemine dokunan her şey trait arkasında; testler `MemoryArchive`
//! kullanır, ağa ya da gerçek zip'e ihtiyaç duymaz.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result, io_err};

/// İçe aktarılacak kayıtların bulunduğu kap: zip, açılmış dizin ya da bellek.
pub trait ExportArchive {
    /// Arşivdeki dosya yollarının tamamı (dizinler hariç).
    fn entry_names(&self) -> Vec<String>;

    /// Tek bir girdinin ham içeriği.
    ///
    /// # Errors
    /// Girdi yoksa ya da okunamazsa [`ErrorKind::Io`] / [`ErrorKind::Archive`].
    fn read_entry(&mut self, name: &str) -> Result<Vec<u8>>;

    /// Rapor ve hata mesajlarında görünecek kaynak adı.
    fn source_label(&self) -> String;
}

/// Bir `.zip` export dosyası.
pub struct ZipArchive {
    path: PathBuf,
    inner: zip::ZipArchive<std::fs::File>,
}

impl ZipArchive {
    /// Zip'i açar ve merkezî dizinini okur.
    ///
    /// # Errors
    /// Dosya açılamazsa ya da geçerli bir zip değilse hata döner.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = std::fs::File::open(&path)
            .map_err(|source| io_err(Stage::ImportRead, path.clone(), source))?;
        let inner = zip::ZipArchive::new(file).map_err(|source| {
            Error::new(
                Stage::ImportRead,
                ErrorKind::Archive {
                    path: path.clone(),
                    source,
                },
            )
        })?;
        Ok(Self { path, inner })
    }
}

impl ExportArchive for ZipArchive {
    fn entry_names(&self) -> Vec<String> {
        self.inner
            .file_names()
            .filter(|name| !name.ends_with('/'))
            .map(ToOwned::to_owned)
            .collect()
    }

    fn read_entry(&mut self, name: &str) -> Result<Vec<u8>> {
        let mut entry = self.inner.by_name(name).map_err(|source| {
            Error::new(
                Stage::ImportRead,
                ErrorKind::Archive {
                    path: self.path.clone(),
                    source,
                },
            )
        })?;
        let mut buf = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .read_to_end(&mut buf)
            .map_err(|source| io_err(Stage::ImportRead, self.path.join(name), source))?;
        Ok(buf)
    }

    fn source_label(&self) -> String {
        self.path.display().to_string()
    }
}

/// Açılmış bir export dizini. Kullanıcı zip'i kendisi açtıysa da çalışsın.
pub struct DirArchive {
    root: PathBuf,
    files: Vec<String>,
}

impl DirArchive {
    /// Dizini özyinelemeli tarar.
    ///
    /// # Errors
    /// Dizin okunamazsa [`ErrorKind::Io`].
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let mut files = Vec::new();
        collect_files(&root, &root, &mut files)?;
        files.sort();
        Ok(Self { root, files })
    }
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).map_err(|source| io_err(Stage::ImportRead, dir, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| io_err(Stage::ImportRead, dir, source))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| io_err(Stage::ImportRead, &path, source))?;
        if file_type.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

impl ExportArchive for DirArchive {
    fn entry_names(&self) -> Vec<String> {
        self.files.clone()
    }

    fn read_entry(&mut self, name: &str) -> Result<Vec<u8>> {
        let path = self.root.join(name);
        std::fs::read(&path).map_err(|source| io_err(Stage::ImportRead, path, source))
    }

    fn source_label(&self) -> String {
        self.root.display().to_string()
    }
}

/// Bellekteki sahte arşiv — testler için.
#[derive(Debug, Default, Clone)]
pub struct MemoryArchive {
    label: String,
    entries: Vec<(String, Vec<u8>)>,
}

impl MemoryArchive {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_entry(mut self, name: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        self.entries.push((name.into(), body.into()));
        self
    }
}

impl ExportArchive for MemoryArchive {
    fn entry_names(&self) -> Vec<String> {
        self.entries.iter().map(|(name, _)| name.clone()).collect()
    }

    fn read_entry(&mut self, name: &str) -> Result<Vec<u8>> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == name)
            .map(|(_, body)| body.clone())
            .ok_or_else(|| {
                Error::new(
                    Stage::ImportRead,
                    ErrorKind::NotFound {
                        what: format!("arşiv girdisi {name}"),
                    },
                )
            })
    }

    fn source_label(&self) -> String {
        self.label.clone()
    }
}
