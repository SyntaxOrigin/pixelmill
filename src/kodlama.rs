//! Kalite kavramının PNG'ye çevrilmesi ve palet kuantizasyonu.
//!
//! ## PNG'de "kalite" nedir?
//!
//! JPEG'de kalite, nicemleme tablolarının ölçeklenmesidir. PNG'de nicemleme
//! tablosu **yoktur**; sıkıştırma tamamen DEFLATE ve filtre seçimine bağlıdır.
//! Bu yüzden PixelMill'in PNG "kalitesi" şu üç kademeli düşüşü ifade eder:
//!
//! | Aralık | Plan | Neden küçülür |
//! |--------|------|----------------|
//! | 90–100 | tam renk (RGB8 / RGBA8 / gri8) | palet indirimi yok |
//! | 70–89  | 8 renk palet | `PLTE` 24 bayt, indeksler 1 piksel/bayt |
//! | 45–69  | 64 renk palet | indeksler 4 piksel/bayt (bit derinliği 2) |
//! | 1–44   | 256 renk palet | `PLTE` 768 bayt, indeksler 1 bayt/piksel |
//!
//! Kalite düştükçe **renk derinliği azalır**; bu, JPEG'de kalite düşürülünce
//! ortaya çıkan bloklanmadan çok daha yumuşak bir geçiştir.
//!
//! ## Kuantizasyon
//!
//! Palet kuantizasyonu **düzgün (uniform) kademelendirmedir** — her kanal
//! bağımsız olarak `2^k` düzeye indirgenir ve `aşama = (R_bit, G_bit, B_bit)`
//! üçlüsüne göre palet 8 / 64 / 256 giriş olur. Bu yöntem öğrenme (medyan-kesme
//! gibi) gerektirmez, **tamamen deterministiktir** ve aynı girdi için bit düzeyinde
//! aynı çıktıyı üretir — raporun "sonuç tekrarlanabilir" kabul kriterinin
//! gereğidir.
//!
//! Şeffaflık varsa palet kullanılamaz (PNG paleti alfasız); bu durumda araç
//! `truecolor+alfa` biçimine düşer ve bunu `KodlamaPlani::not` alanında
//! **açıkça bildirir**.

use crate::gorsel::{Raster, KANAL};
use crate::hata::Hata;
use crate::png::yazma::PngSecenek;

/// Palet kademesi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kademe {
    /// Palet kullanılmaz; gerçek renk (8-bit kanal).
    TamRenk,
    /// 8 girişlik palet (R1, G1, B1 bit).
    Palet8,
    /// 64 girişlik palet (R2, G2, B2 bit).
    Palet64,
    /// 256 girişlik palet (R3, G3, B2 bit).
    Palet256,
}

impl Kademe {
    /// Kalite (0..=100) değerinden kademeyi belirler.
    ///
    /// Eşikler README → `Yapılandırma` bölümünde belgelenmiştir.
    #[must_use]
    pub fn kaliteden(kalite: u8) -> Self {
        match kalite {
            90..=100 => Kademe::TamRenk,
            70..=89 => Kademe::Palet8,
            45..=69 => Kademe::Palet64,
            _ => Kademe::Palet256,
        }
    }

    /// Kademenin palet giriş sayısı (`TamRenk` için 0).
    #[must_use]
    pub fn palet_boyutu(self) -> usize {
        match self {
            Kademe::TamRenk => 0,
            Kademe::Palet8 => 8,
            Kademe::Palet64 => 64,
            Kademe::Palet256 => 256,
        }
    }

    /// Kademenin kısa adı (rapor çıktısı için).
    #[must_use]
    pub fn ad(self) -> &'static str {
        match self {
            Kademe::TamRenk => "tam-renk",
            Kademe::Palet8 => "palet-8",
            Kademe::Palet64 => "palet-64",
            Kademe::Palet256 => "palet-256",
        }
    }

    /// Her kanalın kuantizasyon bit derinliği (R, G, B sırasıyla).
    #[must_use]
    pub fn kanal_bitleri(self) -> Option<(u32, u32, u32)> {
        match self {
            Kademe::TamRenk => None,
            Kademe::Palet8 => Some((1, 1, 1)),
            Kademe::Palet64 => Some((2, 2, 2)),
            Kademe::Palet256 => Some((3, 3, 2)),
        }
    }
}

/// Kalite doğrulaması.
///
/// # Hatalar
///
/// Kalite 0..=100 aralığı dışındaysa `Hata::KaliteGecersiz` döner.
pub fn kaliteyi_dogrula(kalite: u8) -> Result<u8, Hata> {
    if kalite > 100 {
        return Err(Hata::KaliteGecersiz(kalite));
    }
    Ok(kalite)
}

/// Bir kanal örneğini `bit` derinliğine indirger (düzgün kademelendirme).
#[must_use]
pub fn kuantize(deger: u8, bit: u32) -> u8 {
    if bit == 0 || bit >= 8 {
        return deger;
    }
    let adim = 1u8 << (8 - bit);
    // Aşağı yuvarlama: 8-bit eşik değerleri tam olarak korunur.
    deger / adim * adim
}

/// Bir pikseli kademeye göre indis/renk çiftine dönüştürür.
///
/// `(indeks, [r, g, b])` döner; `TamRenk` kademesinde çağrılmaz.
#[must_use]
pub fn pikseli_indisle(piksel: [u8; KANAL], bitler: (u32, u32, u32)) -> (usize, [u8; 3]) {
    let r = kuantize(piksel[0], bitler.0);
    let g = kuantize(piksel[1], bitler.1);
    let b = kuantize(piksel[2], bitler.2);
    let indeks = (usize::from(r >> (8 - bitler.0)) << (bitler.1 + bitler.2))
        | (usize::from(g >> (8 - bitler.1)) << bitler.2)
        | usize::from(b >> (8 - bitler.2));
    (indeks, [r, g, b])
}

/// Bir kademe için palet tablosunu üretir (kademe büyüklüğünde).
#[must_use]
pub fn palet_uret(kademe: Kademe) -> Vec<[u8; 3]> {
    let bitler = match kademe.kanal_bitleri() {
        Some(b) => b,
        None => return Vec::new(),
    };
    let adim_r = 1u8 << (8 - bitler.0);
    let adim_g = 1u8 << (8 - bitler.1);
    let adim_b = 1u8 << (8 - bitler.2);
    let mut palet = Vec::with_capacity(kademe.palet_boyutu());
    let r_sayisi = 1usize << bitler.0;
    let g_sayisi = 1usize << bitler.1;
    let b_sayisi = 1usize << bitler.2;
    for ri in 0..r_sayisi {
        for gi in 0..g_sayisi {
            for bi in 0..b_sayisi {
                let r = (ri as u8) * adim_r;
                let g = (gi as u8) * adim_g;
                let b = (bi as u8) * adim_b;
                palet.push([r, g, b]);
            }
        }
    }
    palet
}

/// Kodlanmış piksellerin biçim tanımı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PikselFormu {
    /// 8-bit gri (`renk_tipi = 0`).
    Gri(Vec<u8>),
    /// 8-bit gri + alfa (`renk_tipi = 4`).
    GriAlfa(Vec<u8>),
    /// 8-bit gerçek renk (`renk_tipi = 2`).
    Rgb(Vec<u8>),
    /// 8-bit gerçek renk + alfa (`renk_tipi = 6`).
    Rgba(Vec<u8>),
    /// Paletli indisler + palet (`renk_tipi = 3`).
    Paletli {
        /// Her piksel için palet indeksi (paketlenmemiş, 1 bayt).
        indisler: Vec<u8>,
        /// Palet renkleri (doğal düzende).
        palet: Vec<[u8; 3]>,
    },
}

impl PikselFormu {
    /// Formun renk tipi kodu.
    #[must_use]
    pub fn renk_tipi(&self) -> u8 {
        match self {
            PikselFormu::Gri(_) => 0,
            PikselFormu::GriAlfa(_) => 4,
            PikselFormu::Rgb(_) => 2,
            PikselFormu::Rgba(_) => 6,
            PikselFormu::Paletli { .. } => 3,
        }
    }

    /// Formun adı (rapor çıktısı için).
    #[must_use]
    pub fn ad(&self) -> &'static str {
        match self {
            PikselFormu::Gri(_) => "gri8",
            PikselFormu::GriAlfa(_) => "gri-alfa8",
            PikselFormu::Rgb(_) => "rgb8",
            PikselFormu::Rgba(_) => "rgba8",
            PikselFormu::Paletli { .. } => "palet",
        }
    }

    /// Formun bir satırındaki bayt sayısı (verilen genişlik için).
    #[must_use]
    pub fn satir_bayt(&self, genislik: u32, bit_derinligi: u8) -> usize {
        match self {
            PikselFormu::Gri(_) => genislik as usize,
            PikselFormu::GriAlfa(_) => genislik as usize * 2,
            PikselFormu::Rgb(_) => genislik as usize * 3,
            PikselFormu::Rgba(_) => genislik as usize * 4,
            PikselFormu::Paletli { .. } => {
                let bit = u64::from(bit_derinligi) * u64::from(genislik);
                bit.div_ceil(8) as usize
            }
        }
    }
}

/// Bir `Raster`i verilen kademeye göre kodlanabilir biçime çevirir.
///
/// # Hatalar
///
/// Kalite geçersizse hata döner.
pub fn form_uret(govde: &Raster, kalite: u8) -> Result<(PikselFormu, Kademe), Hata> {
    kaliteyi_dogrula(kalite)?;
    let kademe = Kademe::kaliteden(kalite);
    let form = if kademe == Kademe::TamRenk {
        tam_renk_formu(govde)
    } else if govde.saydamlik_var() {
        // Palet alfasız: şeffaf görüntüde palete düşülmez, bu durum
        // `KodlamaPlani::not` alanında kullanıcıya bildirilir.
        tam_renk_formu(govde)
    } else {
        let bitler = kademe.kanal_bitleri().unwrap_or((1, 1, 1));
        let palet = palet_uret(kademe);
        let mut indisler = Vec::with_capacity(govde.piksel.len() / KANAL);
        for p in govde.piksel.chunks_exact(KANAL) {
            let (indeks, _) = pikseli_indisle([p[0], p[1], p[2], p[3]], bitler);
            indisler.push(indeks as u8);
        }
        PikselFormu::Paletli { indisler, palet }
    };
    Ok((form, kademe))
}

/// Gövdeyi tam renk biçimlerinden en uygununa çevirir.
///
/// Gri görüntü algılanırsa gri (veya gri+alfa) seçilir; bu, palet kullanılamayan
/// durumlarda bile 8→2 kanallı düşüş sağlar.
#[must_use]
pub fn tam_renk_formu(govde: &Raster) -> PikselFormu {
    let gri = govde.gri_mi();
    let saydam = govde.saydamlik_var();
    match (gri, saydam) {
        (true, false) => PikselFormu::Gri(govde.piksel.iter().step_by(KANAL).copied().collect()),
        (true, true) => PikselFormu::GriAlfa(
            govde
                .piksel
                .chunks_exact(KANAL)
                .flat_map(|p| [p[0], p[3]])
                .collect(),
        ),
        (false, false) => PikselFormu::Rgb(
            govde
                .piksel
                .chunks_exact(KANAL)
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect(),
        ),
        (false, true) => PikselFormu::Rgba(govde.piksel.clone()),
    }
}

/// Kalite ve biçim planı: `PngYazici`ye beslenecek satırları üretir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KodlamaPlani {
    /// `IHDR`/`PLTE` yazımı için gerekli biçim tanımları.
    pub secenek: PngSecenek,
    /// Üretilen pikseller.
    pub form: PikselFormu,
    /// Kullanılan kademe.
    pub kademe: Kademe,
    /// Kullanıcıya gösterilmesi gereken bir uyarı (şeffaflık nedeniyle palet
    /// kullanılamadıysa doludur).
    pub not: Option<String>,
}

impl KodlamaPlani {
    /// Verilen gövdeyi kaliteye göre kodlama planına çevirir.
    ///
    /// # Hatalar
    ///
    /// Kalite geçersizse hata döner.
    pub fn olustur(govde: &Raster, kalite: u8) -> Result<Self, Hata> {
        kaliteyi_dogrula(kalite)?;
        let istenen = Kademe::kaliteden(kalite);
        let (form, kademe) = form_uret(govde, kalite)?;
        let not = if istenen != Kademe::TamRenk
            && istenen.kanal_bitleri().is_some()
            && govde.saydamlik_var()
        {
            Some(format!(
                "{} kalite istenildi ancak görüntü şeffaf olduğu için palet \
                 kullanılamadı; {} biçiminde kodlandı",
                istenen.ad(),
                form.ad()
            ))
        } else {
            None
        };
        let secenek = match &form {
            PikselFormu::Paletli { palet, .. } => {
                PngSecenek::paletli(govde.genislik, govde.yukseklik, palet.clone())?
            }
            diger => PngSecenek::yeni(govde.genislik, govde.yukseklik, 8, diger.renk_tipi())?,
        };
        Ok(Self {
            secenek,
            form,
            kademe,
            not,
        })
    }

    /// `y` numaralı çıktı satırının ham baytlarını üretir.
    #[must_use]
    pub fn satir(&self, y: u32) -> Vec<u8> {
        let g = self.secenek.genislik as usize;
        match &self.form {
            PikselFormu::Gri(v) => v[y as usize * g..(y as usize + 1) * g].to_vec(),
            PikselFormu::GriAlfa(v) => v[y as usize * g * 2..(y as usize + 1) * g * 2].to_vec(),
            PikselFormu::Rgb(v) => v[y as usize * g * 3..(y as usize + 1) * g * 3].to_vec(),
            PikselFormu::Rgba(v) => v[y as usize * g * 4..(y as usize + 1) * g * 4].to_vec(),
            PikselFormu::Paletli { indisler, .. } => paketle_indisler(
                &indisler[y as usize * g..(y as usize + 1) * g],
                self.secenek.bit_derinligi,
            ),
        }
    }
}

/// Palet indislerini PNG'nin istenen bit derinliğine göre paketler.
#[must_use]
pub fn paketle_indisler(indisler: &[u8], bit_derinligi: u8) -> Vec<u8> {
    match bit_derinligi {
        8 => indisler.to_vec(),
        4 => indisler
            .chunks(2)
            .map(|c| (c[0] << 4) | c.get(1).copied().unwrap_or(0))
            .collect(),
        2 => indisler
            .chunks(4)
            .map(|c| {
                let mut b = 0u8;
                for (k, v) in c.iter().take(4).enumerate() {
                    b |= (v & 0x03) << (6 - 2 * k);
                }
                b
            })
            .collect(),
        1 => indisler
            .chunks(8)
            .map(|c| {
                let mut b = 0u8;
                for (k, v) in c.iter().take(8).enumerate() {
                    b |= (v & 0x01) << (7 - k);
                }
                b
            })
            .collect(),
        _ => indisler.to_vec(),
    }
}
