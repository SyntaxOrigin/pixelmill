//! PNG alt modülleri: blok yapısı, filtreler, okuma ve yazma.
//!
//! PixelMill PNG'yi dogrudan uygular; `image` / `png` gibi bir crate
//! kullanilmaz (WORKER_CONTRACT.md 3.2-F). Alt moduller:
//!
//! - [`blok`]: blok (chunk) ayristirma ve CRC-32 hesabi.
//! - [`filtre`]: bes tarama satiri filtresi (None/Sub/Up/Average/Paeth).
//! - [`okuma`]: satir akisiyla cozme, `IHDR` dogrulama, interlacing reddi.
//! - [`yazma`]: satir akisiyla kodlama, filtre secimi, `IDAT` parcalama.
//!
//! DEFLATE/inflate `flate2` (`miniz_oxide`, saf Rust) tarafindan saglanir.

pub mod blok;
pub mod filtre;
pub mod okuma;
pub mod yazma;

pub use blok::PNG_IMZASI;
pub use okuma::{png_bilgi, png_coz, png_dosya_coz, Baslik, PngIcerik};
pub use yazma::{png_kodla, PngSecenek, PngYazici};
