//! Tek bir dosya için uçtan uca iş hattı ve toplu çalıştırıcı.
//!
//! ## Akış
//!
//! ```text
//! bicim tespiti (imza, uzantı değil)
//!   -> PNG? satır akışıyla çöz (CRC doğrulamalı)
//!      -> JPEG? başlık + kuantizasyon oku, piksel işleme reddi
//!   -> kırpma (istenirse)
//!   -> en-boy oranlı ölçekleme / tam boyuta sığdırma (istenirse)
//!   -> hedef bayt araması ya da sabit kalite (moda göre)
//!   -> geçici dosyaya yaz, sonra hedef konuma taşı (atomik)
//!   -> DosyaRaporu üret
//! ```
//!
//! ## Üst üste yazma yasağı
//!
//! Raporun kabul kriteri: "araç hiçbir koşulda mevcut dosyanın üzerine yazmaz".
//! Bunun için (a) kaynak ile çıktı yolu karşılaştırılır ve eşitse hata verilir,
//! (b) çıktı önce `<hedef>.pmtmp` dosyasına yazılır, işlem bitince `rename`
//! ile hedefe taşınır. Yarım kalan bir çıktı hedef konumda **görünmez**.
//!
//! ## Hata izolasyonu
//!
//! Toplu çalıştırmada tek bir dosyanın hatası diğerlerini durdurmaz; hata
//! `DosyaRaporu` içine yazılır ve sayaclar güncellenir.

use std::path::{Path, PathBuf};

use crate::boyut::Filtre;
use crate::gorsel::Raster;
use crate::hata::{io_hata, Hata};
use crate::kodlama::KodlamaPlani;
use crate::kuyruk::{GezintiSecenekleri, KuyrukGirdisi};
use crate::rapor::{Adim, DenemeRaporu, DosyaRaporu, MetaAlan, Rapor};

/// Geçici çıktı dosyasının uzantısı (hedef konumda görünmez).
pub const GECICI_UZANTI: &str = "pmtmp";

/// Bir dosyaya uygulanacak dönüşüm ayarları.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IslemAyar {
    /// Sabit kalite verilecekse bu değer kullanılır.
    pub sabit_kalite: Option<u8>,
    /// Hedef bayt verilecekse ikili arama yapılır.
    pub hedef_bayt: Option<u64>,
    /// En fazla genişlik (en-boy oranı korunur).
    pub en_fazla_genislik: Option<u32>,
    /// En fazla yükseklik (en-boy oranı korunur).
    pub en_fazla_yukseklik: Option<u32>,
    /// Kesin çıktı genişliği (verilirse kırpma/ölçekleme sonrası tam bu olur).
    pub hedef_genislik: Option<u32>,
    /// Kesin çıktı yüksekliği.
    pub hedef_yukseklik: Option<u32>,
    /// Kırpma dikdörtgeni `(x, y, genislik, yukseklik)`.
    pub kirpma: Option<(u32, u32, u32, u32)>,
    /// Kenar boşluğu (piksel).
    pub kenar_boslugu: u32,
    /// Yeniden boyutlandırma filtresi.
    pub filtre: Filtre,
    /// `tEXt` kayıtlarını koru.
    pub metni_koru: bool,
    /// `eXIf` / `APP1` konum verisini koru.
    pub konumu_koru: bool,
}

impl Default for IslemAyar {
    fn default() -> Self {
        Self {
            sabit_kalite: Some(80),
            hedef_bayt: None,
            en_fazla_genislik: None,
            en_fazla_yukseklik: None,
            hedef_genislik: None,
            hedef_yukseklik: None,
            kirpma: None,
            kenar_boslugu: 0,
            filtre: Filtre::Bicubic,
            metni_koru: false,
            konumu_koru: false,
        }
    }
}

/// Bir dosyanın biçim tespiti.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bicim {
    /// PNG imzasıyla başlar.
    Png,
    /// JPEG `SOI` imzasıyla başlar.
    Jpeg,
}

impl Bicim {
    /// Biçimin adı (rapor çıktısı).
    #[must_use]
    pub fn ad(self) -> &'static str {
        match self {
            Bicim::Png => "png",
            Bicim::Jpeg => "jpeg",
        }
    }
}

/// Dosyanın ilk baytlarına bakarak biçimi tespit eder.
///
/// Tespit **imzaya** dayanır, uzantıya değil; yanlış uzantılı ama geçerli bir
/// PNG doğru şekilde işlenir.
///
/// # Hatalar
///
/// Dosya açılamazsa veya tanınmayan bir imza ise hata döner.
pub fn bicim_tespit_et(yol: &Path) -> Result<Bicim, Hata> {
    use std::io::Read;
    let mut dosya = std::fs::File::open(yol).map_err(|e| io_hata(yol, e))?;
    let mut bas = [0u8; 8];
    let okunan = dosya.read(&mut bas).map_err(|e| io_hata(yol, e))?;
    if okunan >= 8 && bas == crate::png::blok::PNG_IMZASI {
        return Ok(Bicim::Png);
    }
    if okunan >= 2 && bas[..2] == crate::jpeg::JPEG_IMZASI {
        return Ok(Bicim::Jpeg);
    }
    Err(Hata::BilinmeyenBicim(yol.display().to_string()))
}

/// Bir dosyayı dönüştürür ve raporunu döndürür.
///
/// `kuru_calistirma` doğruysa hiçbir dosya yazılmaz; yalnızca plan hesaplanır
/// ve rapor `kuru_calistirma` işaretlenir.
///
/// # Hatalar
///
/// Okuma, boyutlandırma veya yazma hatası olursa hata döner. Toplu çalıştırıcı
/// bu hatayı yakalar ve diğer dosyalara devam eder.
pub fn dosyayi_isle(
    kaynak: &Path,
    cikti: &Path,
    ayar: &IslemAyar,
    kuru_calistirma: bool,
) -> Result<DosyaRaporu, Hata> {
    if cikti == kaynak {
        return Err(Hata::CiktiCakismasi(cikti.display().to_string()));
    }
    let bicim = bicim_tespit_et(kaynak)?;
    let mut rapor = DosyaRaporu::yeni(
        &kaynak.display().to_string(),
        &cikti.display().to_string(),
        bicim.ad(),
    );
    rapor.kaynak_bayt = std::fs::metadata(kaynak).map(|m| m.len()).unwrap_or(0);

    let icerik = match bicim {
        Bicim::Png => crate::png::png_dosya_coz(kaynak)?,
        Bicim::Jpeg => {
            // JPEG'de piksel işleme kapsam dışıdır; başlık okunur ve iş
            // bilinçli olarak durdurulur.
            let baslik = crate::jpeg::jpeg_dosya_basligi(kaynak)?;
            if let Some(c) = baslik.cerceve {
                rapor.kaynak_boyut = format!("{}x{}", c.genislik, c.yukseklik);
            }
            for etiket in &baslik.app_etiketleri {
                rapor.meta_alanlari.push(MetaAlan {
                    alan: format!("APP{etiket}"),
                    durum: "raporlandi".to_string(),
                });
            }
            rapor.uyarilar.push(
                "JPEG desteklenir ancak yeniden kodlama kapsam disidir; dosya atlandi".to_string(),
            );
            rapor.hata_sinifi = Some("kodlama".to_string());
            rapor.basarili = false;
            rapor.hata_mesaji = Some(
                Hata::JpegPikselKapsamDisi {
                    islem: "yeniden kodlama".to_string(),
                }
                .to_string(),
            );
            return Ok(rapor);
        }
    };

    rapor.kaynak_boyut = format!("{}x{}", icerik.baslik.genislik, icerik.baslik.yukseklik);
    for kayit in &icerik.metin {
        rapor.meta_alanlari.push(MetaAlan {
            alan: format!("tEXt:{}", kayit.anahtar),
            durum: if ayar.metni_koru {
                "korundu".to_string()
            } else {
                "silindi".to_string()
            },
        });
    }
    if icerik.exif.is_some() {
        rapor.meta_alanlari.push(MetaAlan {
            alan: "eXIf".to_string(),
            durum: if ayar.konumu_koru {
                "korundu".to_string()
            } else {
                "silindi".to_string()
            },
        });
    }

    let mut govde: Raster = icerik.govde;

    // Kırpma
    if let Some((x, y, g, h)) = ayar.kirpma {
        let once = format!("{}x{}", govde.genislik, govde.yukseklik);
        govde = crate::kip::kirp(&govde, x, y, g, h)?;
        rapor.adimlar.push(Adim {
            ad: "kirp".to_string(),
            ayrinti: format!(
                "{once} -> {}x{} (x={x}, y={y})",
                govde.genislik, govde.yukseklik
            ),
        });
    }

    // Ölçekleme
    //
    // `en_fazla_genislik` veya `en_fazla_yukseklik` **tek başına** verilebilir
    // (konsept raporundaki "en fazla 1600 piksel genişlik" kullanımı). Verilmeyen
    // eksen `u32::MAX` ile sınırsız sayılır; `kip::sigdir` oranı `min` ile
    // seçtiği için yalnızca verilen eksen etkili olur ve görüntü asla büyütülmez.
    // `hedef_genislik`/`hedef_yukseklik` ise `cli::ResizeArg::ayara` tarafından
    // birlikte verilmek zorunda tutulur.
    let tam_hedef = ayar.hedef_genislik.is_some() && ayar.hedef_yukseklik.is_some();
    let hedef_g = ayar
        .hedef_genislik
        .or(ayar.en_fazla_genislik)
        .unwrap_or(u32::MAX);
    let hedef_y = ayar
        .hedef_yukseklik
        .or(ayar.en_fazla_yukseklik)
        .unwrap_or(u32::MAX);
    if (hedef_g, hedef_y) != (govde.genislik, govde.yukseklik) {
        let once = format!("{}x{}", govde.genislik, govde.yukseklik);
        govde = if tam_hedef {
            crate::kip::tam_boyuta_sigdir(
                &govde,
                hedef_g,
                hedef_y,
                ayar.filtre,
                [255, 255, 255, 255],
            )?
        } else {
            crate::kip::sigdir(&govde, hedef_g, hedef_y, ayar.filtre)?
        };
        rapor.adimlar.push(Adim {
            ad: "olcekle".to_string(),
            ayrinti: format!("{once} -> {}x{}", govde.genislik, govde.yukseklik),
        });
    }

    // Kenar boşluğu
    if ayar.kenar_boslugu > 0 {
        let once = format!("{}x{}", govde.genislik, govde.yukseklik);
        govde = crate::kip::kenar_boslugu_ekle(&govde, ayar.kenar_boslugu, [255, 255, 255, 255])?;
        rapor.adimlar.push(Adim {
            ad: "kenar_boslugu".to_string(),
            ayrinti: format!("{once} -> {}x{}", govde.genislik, govde.yukseklik),
        });
    }

    // Sıkıştırma
    let (baytlar, plan, kalite, hedef_bayt, hedefe_ulasti, denemeler, uyarilar) =
        sikistir(&govde, ayar)?;
    rapor.kalite = Some(kalite);
    rapor.hedef_bayt = hedef_bayt;
    rapor.hedefe_ulasti = hedefe_ulasti;
    rapor.denemeler = denemeler;
    rapor.cikti_boyut = format!("{}x{}", govde.genislik, govde.yukseklik);
    rapor.cikti_bayt = baytlar.len() as u64;
    rapor.uyarilar.extend(uyarilar);
    if let Some(not) = &plan.not {
        rapor.uyarilar.push(not.clone());
    }
    if ayar.metni_koru {
        for kayit in &icerik.metin {
            if anahtar_gecerli(&kayit.anahtar) {
                rapor.adimlar.push(Adim {
                    ad: "tEXt_koru".to_string(),
                    ayrinti: kayit.anahtar.clone(),
                });
            }
        }
    }
    rapor.adimlar.push(Adim {
        ad: "sikistir".to_string(),
        ayrinti: format!(
            "{} -> {} bayt ({})",
            rapor.kaynak_bayt,
            baytlar.len(),
            plan.form.ad()
        ),
    });

    if !kuru_calistirma {
        atomik_yaz(&baytlar, cikti)?;
    }
    Ok(rapor)
}

/// `tEXt` anahtarının yazılabilir olup olmadığını denetler.
fn anahtar_gecerli(anahtar: &str) -> bool {
    crate::meta::anahtari_dogrula(anahtar).is_ok()
}

/// Sıkıştırma adımı: hedef araması veya sabit kalite.
#[allow(clippy::type_complexity)]
fn sikistir(
    govde: &Raster,
    ayar: &IslemAyar,
) -> Result<
    (
        Vec<u8>,
        KodlamaPlani,
        u8,
        Option<u64>,
        Option<bool>,
        Vec<DenemeRaporu>,
        Vec<String>,
    ),
    Hata,
> {
    if let Some(hedef) = ayar.hedef_bayt {
        let sonuc = crate::hedef::hedef_boyut_arar(
            govde,
            hedef,
            ayar.en_fazla_genislik,
            ayar.en_fazla_yukseklik,
            ayar.filtre,
        )?;
        let denemeler = sonuc
            .denemeler
            .iter()
            .map(|d| DenemeRaporu {
                kalite: d.kalite,
                boyut: d.boyut,
                cozunurluk: format!("{}x{}", d.cozunurluk.0, d.cozunurluk.1),
                hedefin_altinda: d.hedefin_altinda,
            })
            .collect();
        return Ok((
            sonuc.cikti,
            sonuc.plan,
            sonuc.secilen_kalite,
            Some(hedef),
            Some(sonuc.secilen_boyut <= hedef),
            denemeler,
            sonuc.uyarilar,
        ));
    }
    let kalite = ayar.sabit_kalite.unwrap_or(80);
    let (baytlar, plan) = crate::hedef::kodla(govde, kalite)?;
    Ok((baytlar, plan, kalite, None, None, Vec::new(), Vec::new()))
}

/// Baytları geçici dosyaya yazıp hedefe taşır (atomik yazma).
///
/// # Hatalar
///
/// Geçici dosya oluşturulamazsa, yazılamazsa veya taşınamazsa hata döner.
pub fn atomik_yaz(baytlar: &[u8], cikti: &Path) -> Result<(), Hata> {
    let gecici = gecici_yol(cikti);
    if let Some(ust) = cikti.parent() {
        if !ust.as_os_str().is_empty() && !ust.exists() {
            return Err(Hata::CiktiDiziniYok(ust.display().to_string()));
        }
    }
    std::fs::write(&gecici, baytlar).map_err(|e| io_hata(&gecici, e))?;
    std::fs::rename(&gecici, cikti).map_err(|e| io_hata(cikti, e))
}

/// Çıktı yolundan geçici dosya yolu üretir.
#[must_use]
pub fn gecici_yol(cikti: &Path) -> PathBuf {
    let mut ad = cikti
        .file_name()
        .map_or_else(|| "cikti".into(), |n| n.to_os_string());
    ad.push(".");
    ad.push(GECICI_UZANTI);
    cikti.with_file_name(ad)
}

/// Bir klasördeki tüm dosyaları dönüştürür ve toplu rapor üretir.
///
/// Tek bir dosyanın hatası diğerlerini durdurmaz. `kuru_calistirma`
/// yapılıyorsa hiçbir dosya yazılmaz.
///
/// # Hatalar
///
/// Kaynak yol okunamazsa hata döner. Dosya bazlı hatalar rapora yazılır.
pub fn klasoru_isle(
    kaynak: &Path,
    cikti_dizini: &Path,
    ayar: &IslemAyar,
    kuru_calistirma: bool,
) -> Result<Rapor, Hata> {
    let secenek = GezintiSecenekleri {
        atlanacak_dizin: Some(cikti_dizini.to_path_buf()),
        gizlilere_dahil: false,
    };
    let girdiler = crate::kuyruk::kuyrugu_olustur(kaynak, &secenek)?;
    let mut rapor = Rapor::yeni(kuru_calistirma);
    if !kuru_calistirma && !cikti_dizini.exists() {
        std::fs::create_dir_all(cikti_dizini).map_err(|e| io_hata(cikti_dizini, e))?;
    }
    for girdi in girdiler {
        let cikti = cikti_dizini.join(girdi.dosya_adi());
        let tek = dosyayi_isle(&girdi.yol, &cikti, ayar, kuru_calistirma);
        match tek {
            Ok(r) => rapor.ekle(r),
            Err(hata) => {
                let mut r = DosyaRaporu::yeni(
                    &girdi.yol.display().to_string(),
                    &cikti.display().to_string(),
                    "-",
                );
                r.hatayi_isaretle(&hata);
                rapor.ekle(r);
            }
        }
    }
    Ok(rapor)
}

/// Bir kuyruk girdisi listesini sırayla dönüştürür (dizin gezintisi yapmaz).
///
/// # Hatalar
///
/// Çıktı dizini oluşturulamazsa hata döner. Dosya bazlı hatalar rapora yazılır.
pub fn kuyrugu_isle(
    girdiler: &[KuyrukGirdisi],
    cikti_dizini: &Path,
    ayar: &IslemAyar,
    kuru_calistirma: bool,
) -> Result<Rapor, Hata> {
    let mut rapor = Rapor::yeni(kuru_calistirma);
    if !kuru_calistirma && !cikti_dizini.exists() {
        std::fs::create_dir_all(cikti_dizini).map_err(|e| io_hata(cikti_dizini, e))?;
    }
    for girdi in girdiler {
        let cikti = cikti_dizini.join(girdi.dosya_adi());
        match dosyayi_isle(&girdi.yol, &cikti, ayar, kuru_calistirma) {
            Ok(r) => rapor.ekle(r),
            Err(hata) => {
                let mut r = DosyaRaporu::yeni(
                    &girdi.yol.display().to_string(),
                    &cikti.display().to_string(),
                    "-",
                );
                r.hatayi_isaretle(&hata);
                rapor.ekle(r);
            }
        }
    }
    Ok(rapor)
}
