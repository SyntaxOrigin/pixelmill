//! Toplu iş kuyruğu: klasör gezintesi ve dosya listesi.
//!
//! ## Neden `walkdir` yok?
//!
//! `walkdir` ve `notify` bağımlılık politikasıyla yasaktır
//! (WORKER_CONTRACT.md 3.2-F). Gezinti bu modülün **kendi özyinelemeli**
//! `std::fs::read_dir` uygulamasıdır.
//!
//! ## Filtreler
//!
//! - **uzantı**: `.png`, `.jpg`, `.jpeg` (büyük/küçük harf duyarsız).
//! - **gizli dosya**: adı `.` ile başlayanlar ve Windows'ta `FILE_ATTRIBUTE_HIDDEN`
//!   bayrağı taşıyanlar atlanır.
//! - **derinlik**: `EN_FAZLA_DERINLIK` (32) aşılmaz; simge bağlantısı seli ve
//!   sonsuz döngüye karşı yüzey sınırıdır.
//! - **çıktı dizini**: kaynak ağacının içindeyse atlanır (kendi çıktımızı
//!   girdi olarak saymamak için).
//!
//! Kuyruk **sıralı** ve **tekrarlanabilir** olsun diye yol metnine göre sıralanır.

use std::path::{Path, PathBuf};

use crate::hata::{io_hata, Hata};
use crate::sinir::EN_FAZLA_DERINLIK;

/// Kabul edilen dosya uzantıları.
pub const KABUL_EDILEN_UZANTILAR: [&str; 3] = ["png", "jpg", "jpeg"];

/// Bir kuyruk girdisi.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KuyrukGirdisi {
    /// Dosyanın tam yolu.
    pub yol: PathBuf,
    /// Dosyanın bayt cinsinden boyutu (`0` bilinmiyor demektir).
    pub boyut: u64,
}

impl KuyrukGirdisi {
    /// Yeni bir girdi oluşturur.
    #[must_use]
    pub fn yeni(yol: PathBuf, boyut: u64) -> Self {
        Self { yol, boyut }
    }

    /// Dosya adını döndürür.
    #[must_use]
    pub fn dosya_adi(&self) -> String {
        self.yol.file_name().map_or_else(
            || self.yol.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }
}

/// Gezinti seçenekleri.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GezintiSecenekleri {
    /// Kaynak ağacının içinde yer alan ve atlanacak çıktı dizini.
    pub atlanacak_dizin: Option<PathBuf>,
    /// Gizli dosyaları dahil et (varsayılan: hayır).
    pub gizlilere_dahil: bool,
}

/// Bir dosyanın uzantısını küçük harfe döndürür (nokta hariç).
#[must_use]
pub fn uzanti(yol: &Path) -> String {
    yol.extension()
        .map_or_else(String::new, |e| e.to_string_lossy().to_ascii_lowercase())
}

/// Uzantının kabul edilip edilmediğini belirler.
///
/// # Hatalar
///
/// Uzantı kabul edilmiyorsa `Hata::UzantiDesteklenmiyor` döner.
pub fn uzantiyi_dogrula(yol: &Path) -> Result<String, Hata> {
    let u = uzanti(yol);
    if KABUL_EDILEN_UZANTILAR.contains(&u.as_str()) {
        Ok(u)
    } else {
        let liste = KABUL_EDILEN_UZANTILAR.join(", ");
        Err(Hata::UzantiDesteklenmiyor {
            uzanti: u,
            desteklenen: liste,
        })
    }
}

/// Bir adın gizli sayılıp sayılmayacağını belirler.
///
/// Unix'te `.` ile başlayan adlar; Windows'ta `FILE_ATTRIBUTE_HIDDEN`
/// bayrağı taşıyan dosyalar gizlidir.
///
/// # Hatalar
///
/// `std::fs::metadata` hata döndürürse hata döner.
pub fn gizli_mi(yol: &Path) -> Result<bool, Hata> {
    if yol
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.'))
    {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let meta = std::fs::metadata(yol).map_err(|e| io_hata(yol, e))?;
        Ok(meta.file_attributes() & 0x2 != 0)
    }
    #[cfg(not(windows))]
    {
        let _ = yol;
        Ok(false)
    }
}

/// Bir kaynak (dosya veya klasör) için kuyruk girdilerini toplar.
///
/// Klasör verildiyse özyinelemeli olarak gezilir; dosya verildiyse tek
/// girdi döner. Sonuç yol metnine göre sıralanır (tekrarlanabilirlik).
///
/// # Hatalar
///
/// Kaynak mevcut değilse, bir dosya değilse veya yol okunamazsa hata döner.
/// Tek bir alt dizinin okunamaması tüm gezintiyi düşürmez; o dizin atlanır.
pub fn kuyrugu_olustur(
    kaynak: &Path,
    secenek: &GezintiSecenekleri,
) -> Result<Vec<KuyrukGirdisi>, Hata> {
    let meta = std::fs::metadata(kaynak).map_err(|e| io_hata(kaynak, e))?;
    let mut girdiler: Vec<KuyrukGirdisi> = Vec::new();
    if meta.is_file() {
        if let Some(girdi) = dosya_girdisi(kaynak, secenek)? {
            girdiler.push(girdi);
        }
        return Ok(girdiler);
    }
    gez(kaynak, 0, secenek, &mut girdiler);
    girdiler.sort_by(|a, b| a.yol.cmp(&b.yol));
    Ok(girdiler)
}

/// Tek bir dosya için kuyruk girdisi üretir; filtreye uymazsa `None`.
fn dosya_girdisi(yol: &Path, secenek: &GezintiSecenekleri) -> Result<Option<KuyrukGirdisi>, Hata> {
    if !secenek.gizlilere_dahil && gizli_mi(yol)? {
        return Ok(None);
    }
    if uzantiyi_dogrula(yol).is_err() {
        return Ok(None);
    }
    let boyut = std::fs::metadata(yol).map(|m| m.len()).unwrap_or(0);
    Ok(Some(KuyrukGirdisi::yeni(yol.to_path_buf(), boyut)))
}

/// Dizini özyinelemeli gezer.
fn gez(
    dizin: &Path,
    derinlik: usize,
    secenek: &GezintiSecenekleri,
    girdiler: &mut Vec<KuyrukGirdisi>,
) {
    if derinlik > EN_FAZLA_DERINLIK {
        return;
    }
    let okunan = match std::fs::read_dir(dizin) {
        Ok(o) => o,
        // Okunamayan dizin tüm gezintiyi düşürmez.
        Err(_) => return,
    };
    for giris in okunan.flatten() {
        let yol = giris.path();
        if let Some(atlanacak) = &secenek.atlanacak_dizin {
            if yol == *atlanacak {
                continue;
            }
        }
        let tip = match giris.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if tip.is_dir() {
            gez(&yol, derinlik + 1, secenek, girdiler);
        } else if tip.is_file() {
            match dosya_girdisi(&yol, secenek) {
                Ok(Some(g)) => girdiler.push(g),
                Ok(None) => {}
                Err(_) => continue,
            }
        }
    }
}
