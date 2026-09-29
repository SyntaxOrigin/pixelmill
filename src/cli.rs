//! `clap` ile tanımlanan komut satırı arayüzü.
//!
//! Alt komutlar:
//!
//! - `info <dosya>` — biçim, boyut, renk tipi, blok/marker listesi, meta veri
//! - `convert <girdi> <cikti>` — kalite ile yeniden kodlama (hedef boyut seçeneği ile)
//! - `resize <girdi> <cikti> --genislik --yukseklik` — yeniden boyutlandırma
//! - `optimize <girdi> <cikti> --hedef-boyut` — hedef bayta ikili arama
//! - `batch <kaynak> --cikti-dizini` — klasördeki çoklu dosya (kuru çalıştırma destekli)
//!
//! Tüm alt komutlar `--rapor <yol>` ile JSON raporu dosyasına yazar ve `--json`
//! ile raporu standart çıktıya basar.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::boyut::Filtre;
use crate::hata::Hata;
use crate::islem::IslemAyar;

/// Yeniden boyutlandırma filtresi için `clap` değer listesi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FiltreArg {
    /// Alan (kutu) ortalaması — küçültmede en iyisi.
    Kutu,
    /// En yakın komşu.
    EnYakin,
    /// Bilineer.
    Bilinear,
    /// Bicubic (Catmull-Rom).
    Bicubic,
}

impl From<FiltreArg> for Filtre {
    fn from(deger: FiltreArg) -> Self {
        match deger {
            FiltreArg::Kutu => Filtre::Kutu,
            FiltreArg::EnYakin => Filtre::EnYakin,
            FiltreArg::Bilinear => Filtre::Bilinear,
            FiltreArg::Bicubic => Filtre::Bicubic,
        }
    }
}

impl TryFrom<&str> for FiltreArg {
    type Error = Hata;

    fn try_from(deger: &str) -> Result<Self, Self::Error> {
        match deger {
            "kutu" | "box" | "area" => Ok(FiltreArg::Kutu),
            "en-yakin" | "nearest" => Ok(FiltreArg::EnYakin),
            "bilinear" => Ok(FiltreArg::Bilinear),
            "bicubic" => Ok(FiltreArg::Bicubic),
            diger => Err(Hata::AyarGecersiz {
                ad: "--filtre".to_string(),
                deger: diger.to_string(),
            }),
        }
    }
}

/// PixelMill — toplu görsel dönüştürücü, yeniden boyutlandırıcı ve sıkıştırıcı.
///
/// PNG okuma/yazma, JPEG başlık/kuantizasyon okuma, yeniden boyutlandırma ve
/// hedef bayt için ikili arama desteklenir.
#[derive(Debug, Parser)]
#[command(name = "pixelmill", version, about, long_about = None)]
pub struct Cli {
    /// Ne yapılacağını belirleyen alt komut.
    #[command(subcommand)]
    pub komut: Komut,
}

/// Alt komutlar.
#[derive(Debug, Subcommand)]
pub enum Komut {
    /// Bir görselin biçimini, boyutunu ve gömülü verilerini bildirir.
    Info(InfoArg),
    /// PNG'yi yeniden kodlar (kalite veya hedef bayt ile).
    Convert(KaynakCiktiArg),
    /// Görseli yeniden boyutlandırır.
    Resize(ResizeArg),
    /// Görseli hedef bayta sığdırmak için ikili arama yapar.
    Optimize(KaynakCiktiArg),
    /// Bir klasördeki tüm desteklenen dosyaları dönüştürür.
    Batch(BatchArg),
}

/// `info` alt komutunun seçenekleri.
#[derive(Debug, Args)]
pub struct InfoArg {
    /// İncelenecek dosya.
    #[arg(value_name = "DOSYA")]
    pub dosya: PathBuf,
    /// Raporu JSON olarak ekrana basar.
    #[arg(long)]
    pub json: bool,
}

/// Kaynak/çıktı çifti ve ortak dönüşüm seçenekleri.
#[derive(Debug, Args)]
pub struct KaynakCiktiArg {
    /// Kaynak PNG dosyası.
    #[arg(value_name = "GIRDI")]
    pub girdi: PathBuf,
    /// Yazılacak çıktı dosyası.
    #[arg(value_name = "CIKTI")]
    pub cikti: PathBuf,
    /// Sabit kalite (0..=100). Varsayılan: 80.
    #[arg(long, value_name = "0-100")]
    pub kalite: Option<u8>,
    /// Hedef bayt sayısı; verilirse ikili arama yapılır.
    #[arg(long, value_name = "BAYT")]
    pub hedef_boyut: Option<u64>,
    /// En-boy oranını koruyarak en fazla bu genişliğe küçültür.
    #[arg(long, value_name = "PIKSEL")]
    pub en_fazla_genislik: Option<u32>,
    /// En-boy oranını koruyarak en fazla bu yüksekliğe küçültür.
    #[arg(long, value_name = "PIKSEL")]
    pub en_fazla_yukseklik: Option<u32>,
    /// Yeniden boyutlandırma filtresi.
    #[arg(long, value_enum, default_value_t = FiltreArg::Bicubic)]
    pub filtre: FiltreArg,
    /// `tEXt` metin alanlarını koru (varsayılan: sil).
    #[arg(long)]
    pub metni_koru: bool,
    /// `eXIf` / EXIF konum verisini koru (varsayılan: sil).
    #[arg(long)]
    pub konumu_koru: bool,
    /// JSON rapor dosyası yolu.
    #[arg(long, value_name = "DOSYA")]
    pub rapor: Option<PathBuf>,
    /// Raporu ekrana basar.
    #[arg(long)]
    pub json: bool,
    /// Yalnızca planı hesaplar, dosya yazmaz.
    #[arg(long)]
    pub kuru_calistir: bool,
}

impl KaynakCiktiArg {
    /// Ortak seçeneklerden [`IslemAyar`] üretir.
    ///
    /// # Hatalar
    ///
    /// Kalite 0..=100 dışındaysa veya hedef bayt sıfırsa hata döner.
    pub fn ayara(&self) -> Result<IslemAyar, Hata> {
        if let Some(k) = self.kalite {
            crate::kodlama::kaliteyi_dogrula(k)?;
        }
        if self.hedef_boyut == Some(0) {
            return Err(Hata::HedefBoyutGecersiz(0));
        }
        Ok(IslemAyar {
            sabit_kalite: if self.hedef_boyut.is_some() {
                None
            } else {
                Some(self.kalite.unwrap_or(80))
            },
            hedef_bayt: self.hedef_boyut,
            en_fazla_genislik: self.en_fazla_genislik,
            en_fazla_yukseklik: self.en_fazla_yukseklik,
            hedef_genislik: None,
            hedef_yukseklik: None,
            kirpma: None,
            kenar_boslugu: 0,
            filtre: Filtre::from(self.filtre),
            metni_koru: self.metni_koru,
            konumu_koru: self.konumu_koru,
        })
    }
}

/// `resize` alt komutunun seçenekleri.
#[derive(Debug, Args)]
pub struct ResizeArg {
    /// Kaynak PNG dosyası.
    #[arg(value_name = "GIRDI")]
    pub girdi: PathBuf,
    /// Yazılacak çıktı dosyası.
    #[arg(value_name = "CIKTI")]
    pub cikti: PathBuf,
    /// Hedef genişlik (piksel).
    #[arg(long, value_name = "PIKSEL")]
    pub genislik: Option<u32>,
    /// Hedef yükseklik (piksel).
    #[arg(long, value_name = "PIKSEL")]
    pub yukseklik: Option<u32>,
    /// En-boy oranını koruyarak en fazla bu genişliğe küçültür.
    #[arg(long, value_name = "PIKSEL")]
    pub en_fazla_genislik: Option<u32>,
    /// En-boy oranını koruyarak en fazla bu yüksekliğe küçültür.
    #[arg(long, value_name = "PIKSEL")]
    pub en_fazla_yukseklik: Option<u32>,
    /// Üç değer (`x,y,g`) olarak da verilebilir; dördüncü değer ötomatik hesaplanır.
    #[arg(long, value_name = "X,Y,G")]
    pub kirpma: Option<String>,
    /// Kenar boşluğu (piksel).
    #[arg(long, default_value_t = 0, value_name = "PIKSEL")]
    pub kenar_boslugu: u32,
    /// Kalite (0..=100). Varsayılan: 80.
    #[arg(long, value_name = "0-100")]
    pub kalite: Option<u8>,
    /// Yeniden boyutlandırma filtresi.
    #[arg(long, value_enum, default_value_t = FiltreArg::Bicubic)]
    pub filtre: FiltreArg,
    /// `tEXt` metin alanlarını koru.
    #[arg(long)]
    pub metni_koru: bool,
    /// `eXIf` / EXIF konum verisini koru.
    #[arg(long)]
    pub konumu_koru: bool,
    /// JSON rapor dosyası yolu.
    #[arg(long, value_name = "DOSYA")]
    pub rapor: Option<PathBuf>,
    /// Raporu ekrana basar.
    #[arg(long)]
    pub json: bool,
    /// Yalnızca planı hesaplar, dosya yazmaz.
    #[arg(long)]
    pub kuru_calistir: bool,
}

impl ResizeArg {
    /// Seçeneklerden [`IslemAyar`] üretir.
    ///
    /// # Hatalar
    ///
    /// `--kirpma` çözümlenemezse veya kalite geçersizse hata döner.
    pub fn ayara(&self) -> Result<IslemAyar, Hata> {
        if let Some(k) = self.kalite {
            crate::kodlama::kaliteyi_dogrula(k)?;
        }
        let kirpma = match &self.kirpma {
            None => None,
            Some(metin) => Some(kirpma_ayir(metin)?),
        };
        if (self.genislik.is_none()) != (self.yukseklik.is_none()) {
            return Err(Hata::AyarGecersiz {
                ad: "--genislik/--yukseklik".to_string(),
                deger: "her ikisi de verilmelidir".to_string(),
            });
        }
        Ok(IslemAyar {
            sabit_kalite: Some(self.kalite.unwrap_or(80)),
            hedef_bayt: None,
            en_fazla_genislik: self.en_fazla_genislik,
            en_fazla_yukseklik: self.en_fazla_yukseklik,
            hedef_genislik: self.genislik,
            hedef_yukseklik: self.yukseklik,
            kirpma,
            kenar_boslugu: self.kenar_boslugu,
            filtre: Filtre::from(self.filtre),
            metni_koru: self.metni_koru,
            konumu_koru: self.konumu_koru,
        })
    }
}

/// `--kirpma` değerini `x,y,g` biçiminden ayrıştırır.
///
/// # Hatalar
///
/// Biçim yanlışsa veya sayılar sıfır/taşmışsa hata döner.
pub fn kirpma_ayir(metin: &str) -> Result<(u32, u32, u32, u32), Hata> {
    let parcalar: Vec<&str> = metin.split(',').map(str::trim).collect();
    if parcalar.len() != 3 {
        return Err(Hata::AyarGecersiz {
            ad: "--kirpma".to_string(),
            deger: metin.to_string(),
        });
    }
    let say = |i: usize| -> Result<u32, Hata> {
        parcalar[i].parse::<u32>().map_err(|_| Hata::AyarGecersiz {
            ad: "--kirpma".to_string(),
            deger: metin.to_string(),
        })
    };
    let x = say(0)?;
    let y = say(1)?;
    let g = say(2)?;
    if g == 0 {
        return Err(Hata::AyarGecersiz {
            ad: "--kirpma".to_string(),
            deger: metin.to_string(),
        });
    }
    Ok((x, y, g, g))
}

/// `batch` alt komutunun seçenekleri.
#[derive(Debug, Args)]
pub struct BatchArg {
    /// Kaynak dosya veya klasör.
    #[arg(value_name = "KAYNAK")]
    pub kaynak: PathBuf,
    /// Çıktı klasörü.
    #[arg(long, value_name = "DIZIN")]
    pub cikti_dizini: PathBuf,
    /// Hedef bayt; verilirse her dosya için ikili arama yapılır.
    #[arg(long, value_name = "BAYT")]
    pub hedef_boyut: Option<u64>,
    /// Sabit kalite (0..=100). Varsayılan: 80.
    #[arg(long, value_name = "0-100")]
    pub kalite: Option<u8>,
    /// En-boy oranını koruyarak en fazla bu genişliğe küçültür.
    #[arg(long, value_name = "PIKSEL")]
    pub en_fazla_genislik: Option<u32>,
    /// En-boy oranını koruyarak en fazla bu yüksekliğe küçültür.
    #[arg(long, value_name = "PIKSEL")]
    pub en_fazla_yukseklik: Option<u32>,
    /// Gizli dosyaları da dahil et.
    #[arg(long)]
    pub gizlileri_dahil: bool,
    /// Yalnızca listeler, hiçbir dosya yazmaz.
    #[arg(long)]
    pub kuru_calistir: bool,
    /// JSON rapor dosyası yolu.
    #[arg(long, value_name = "DOSYA")]
    pub rapor: Option<PathBuf>,
    /// Raporu ekrana basar.
    #[arg(long)]
    pub json: bool,
}

impl BatchArg {
    /// Seçeneklerden [`IslemAyar`] üretir.
    ///
    /// # Hatalar
    ///
    /// Kalite veya hedef bayt geçersizse hata döner.
    pub fn ayara(&self) -> Result<IslemAyar, Hata> {
        if let Some(k) = self.kalite {
            crate::kodlama::kaliteyi_dogrula(k)?;
        }
        if self.hedef_boyut == Some(0) {
            return Err(Hata::HedefBoyutGecersiz(0));
        }
        Ok(IslemAyar {
            sabit_kalite: if self.hedef_boyut.is_some() {
                None
            } else {
                Some(self.kalite.unwrap_or(80))
            },
            hedef_bayt: self.hedef_boyut,
            en_fazla_genislik: self.en_fazla_genislik,
            en_fazla_yukseklik: self.en_fazla_yukseklik,
            hedef_genislik: None,
            hedef_yukseklik: None,
            kirpma: None,
            kenar_boslugu: 0,
            filtre: Filtre::Bicubic,
            metni_koru: false,
            konumu_koru: false,
        })
    }
}
