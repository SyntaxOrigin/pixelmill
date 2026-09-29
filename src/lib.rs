//! PixelMill — toplu görsel dönüştürücü, yeniden boyutlandırıcı ve sıkıştırıcı.
//!
//! Bu kitaplık (crate) komut satırı kabuğundan (`main.rs`) ayrıdır; çekirdek
//! mantığı test edilebilir olması için burada toplanır. Tüm kaynak kod
//! `#![forbid(unsafe_code)]` ile derlenir ve harici C kütüphanesi bağlamaz.
//!
//! ## Modüller
//!
//! | Modül | Sorumluluk |
//! |-------|-------------|
//! | [`hata`] | Hata türü ve `Display` uygulaması |
//! | [`sinir`] | Güvenlik üst sınırları (bellek bombası koruması) |
//! | [`gorsel`] | RGBA gövde ve sıralı satır okuyucu soyutlaması |
//! | [`png`] | PNG blok/filtre/okuma/yazma (kendi kodumuz) |
//! | [`jpeg`] | JPEG başlık ve kuantizasyon tablosu okuyucu (baseline) |
//! | [`meta`] | Gömülü metin ve EXIF alanlarının okunması/temizlenmesi |
//! | [`boyut`] | Kutu, en yakın, bilineer, bicubic yeniden boyutlandırma |
//! | [`kip`] | Kırpma ve kenar boşluğu ekleme |
//! | [`kodlama`] | Kalite → biçim planı ve palet kuantizasyonu |
//! | [`hedef`] | Hedef bayt için ikili arama |
//! | [`kuyruk`] | Klasör gezintesi ve dosya kuyruğu |
//! | [`islem`] | Tek dosya için uçtan uca iş hattı |
//! | [`rapor`] | Serileştirilebilir rapor yapıları |
//! | [`cli`] | `clap` alt komut tanımları |
//!
//! ## Kapsam dışı (bilinçli)
//!
//! WebP, AVIF ve JPEG2000 desteklenmez; JPEG **yeniden kodlanmaz** (yalnızca
//! başlık/kuantizasyon okunur). Gerekçeler `README.md` → `Bilinen Sınırlamalar`
//! bölümündedir.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![warn(clippy::unwrap_used, clippy::expect_used)]

pub mod boyut;
pub mod cli;
pub mod gorsel;
pub mod hata;
pub mod hedef;
pub mod islem;
pub mod jpeg;
pub mod kip;
pub mod kodlama;
pub mod kuyruk;
pub mod meta;
pub mod png;
pub mod rapor;
pub mod sinir;

pub use boyut::Filtre;
pub use gorsel::Raster;
pub use hata::Hata;
pub use rapor::{DosyaRaporu, Rapor};

/// PixelMill sürümü (`Cargo.toml` ile eşleşir).
pub const SURUM: &str = env!("CARGO_PKG_VERSION");

/// Araç adı (log ve hata mesajlarında kullanılır).
pub const ARAK_ADI: &str = "pixelmill";

/// Yalnızca test derlemelerinde bulunan ortak yardımcılar.
///
/// **Neden `unwrap()`/`expect()` kullanılmıyor?** `lib.rs` seviyesinde
/// `#![warn(clippy::unwrap_used, clippy::expect_used)]` etkin ve kalite kapısı
/// `clippy --all-targets -- -D warnings` ile çalıştırır; test kodunda bile
/// `unwrap` kullanmak kapıyı kırardı. Bu yardımcı aynı işi, hatayı `Debug`
/// çıktısıyla panik mesajına taşıyarak yapar.
#[cfg(test)]
pub(crate) mod test_yardimci {
    /// `Ok` değerini döndürür, `Err` durumunda hatayı yazdırıp panikler.
    ///
    /// `Result::unwrap` ile aynı davranış, ancak `clippy::unwrap_used`
    /// uyarısı üretmez.
    pub(crate) fn ac<T, E: std::fmt::Debug>(sonuc: Result<T, E>) -> T {
        match sonuc {
            Ok(deger) => deger,
            Err(hata) => panic!("beklenmeyen hata: {hata:?}"),
        }
    }
}
