//! Serileştirilebilir çıktı raporları (MANIFEST kart 04, madde 6).
//!
//! Her dosya için: kaynak/çıktı yolu, kaynak ve çıktı boyutu, seçilen kalite,
//! üretilen bayt sayısı, uygulanan adımlar, hedef boyut aramasının denemeleri,
//! silinen ve korunan meta veri alanları, uyarılar ve hata sınıfı üretilir.
//!
//! Rapor `serde` türetilmiş yapılardır; `serde_json` ile `pixelmill --rapor`
//! seçeneğiyle disa aktarılır veya `--json` ile standart çıktıya basılır.

use serde::Serialize;

use crate::hata::Hata;

/// Bir kalite denemesinin rapor satırı.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DenemeRaporu {
    /// Denenen kalite.
    pub kalite: u8,
    /// Üretilen bayt sayısı.
    pub boyut: u64,
    /// Denemedeki çözünürlük.
    pub cozunurluk: String,
    /// Hedefin altında kaldı mı?
    pub hedefin_altinda: bool,
}

/// Görüntünün hangi işlemlerden geçtiğini anlatan adım listesi.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Adim {
    /// Adımın kısa adı (ör. `"kirp"`, `"olcekle"`, `"sikistir"`).
    pub ad: String,
    /// Adımın uygulanmasından önceki/sonraki ölçü (`"1024x768 -> 640x480"`).
    pub ayrinti: String,
}

/// Bir meta veri alanının akıbeti.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MetaAlan {
    /// Alanın adı (ör. `"tEXt:Comment"` veya `"EXIF:Artist"`).
    pub alan: String,
    /// `"korundu"`, `"silindi"` veya `"raporlandi"`.
    pub durum: String,
}

/// Tek bir dosyanın işleniş raporu.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DosyaRaporu {
    /// Kaynak dosyanın yolu.
    pub kaynak: String,
    /// Üretilen çıktının yolu (`--kuru-calistir` durumunda hedef yol).
    pub cikti: String,
    /// Kaynak biçimi (`"png"` veya `"jpeg"`).
    pub bicim: String,
    /// Kaynak boyutu.
    pub kaynak_boyut: String,
    /// Çıktı boyutu.
    pub cikti_boyut: String,
    /// Kaynak dosyanın bayt cinsinden boyutu.
    pub kaynak_bayt: u64,
    /// Çıktının bayt cinsinden boyutu.
    pub cikti_bayt: u64,
    /// Kullanılan kalite (`None` ise kalite araması yapılmadı).
    pub kalite: Option<u8>,
    /// İstenen hedef bayt (varsa).
    pub hedef_bayt: Option<u64>,
    /// Hedefe ulaşıldı mı?
    pub hedefe_ulasti: Option<bool>,
    /// Uygulanan işlem adımları.
    pub adimlar: Vec<Adim>,
    /// Hedef boyut aramasının denemeleri (arama yapıldıysa).
    pub denemeler: Vec<DenemeRaporu>,
    /// Meta veri alanlarının akıbeti.
    pub meta_alanlari: Vec<MetaAlan>,
    /// Kullanıcıya gösterilecek uyarılar.
    pub uyarilar: Vec<String>,
    /// Başarılı mı?
    pub basarili: bool,
    /// Başarısızsa hata sınıfı (`"okuma"`, `"kodlama"`, `"boyut"`, `"yazma"`).
    pub hata_sinifi: Option<String>,
    /// Başarısızsa hata mesajı.
    pub hata_mesaji: Option<String>,
}

impl DosyaRaporu {
    /// Başarılı bir iş için boş bir rapor iskeleti oluşturur.
    #[must_use]
    pub fn yeni(kaynak: &str, cikti: &str, bicim: &str) -> Self {
        Self {
            kaynak: kaynak.to_string(),
            cikti: cikti.to_string(),
            bicim: bicim.to_string(),
            kaynak_boyut: "-".to_string(),
            cikti_boyut: "-".to_string(),
            kaynak_bayt: 0,
            cikti_bayt: 0,
            kalite: None,
            hedef_bayt: None,
            hedefe_ulasti: None,
            adimlar: Vec::new(),
            denemeler: Vec::new(),
            meta_alanlari: Vec::new(),
            uyarilar: Vec::new(),
            basarili: true,
            hata_sinifi: None,
            hata_mesaji: None,
        }
    }

    /// Bir hata ile işaretler (rapor hâlâ serileştirilebilir kalır).
    pub fn hatayi_isaretle(&mut self, hata: &Hata) {
        self.basarili = false;
        self.hata_sinifi = Some(hata_sinifi(hata).to_string());
        self.hata_mesaji = Some(hata.to_string());
    }

    /// Başarı özeti satırı (insan tarafından okunur).
    #[must_use]
    pub fn ozet(&self) -> String {
        if self.basarili {
            format!(
                "OK   {} -> {} ({} bayt, kalite {})",
                self.kaynak,
                self.cikti,
                self.cikti_bayt,
                self.kalite
                    .map_or_else(|| "-".to_string(), |k| k.to_string())
            )
        } else {
            format!(
                "HATA {} ({}) - {}",
                self.kaynak,
                self.hata_sinifi.as_deref().unwrap_or("?"),
                self.hata_mesaji.as_deref().unwrap_or("?")
            )
        }
    }
}

/// Hatanın hangi sınıfa ait olduğunu belirler (rapor bölüm 07: "dört sınıf").
///
/// `"okuma" | "kodlama" | "boyut" | "meta" | "kuyruk" | "yazma" | "ayar"`
#[must_use]
pub fn hata_sinifi(hata: &Hata) -> &'static str {
    match hata {
        Hata::Dosya { .. } => "yazma",
        Hata::PngImzasiBozuk
        | Hata::PngBlokBozuk { .. }
        | Hata::PngCrcBozuk { .. }
        | Hata::PngBaslikBozuk { .. }
        | Hata::PngRenkTipiGecersiz { .. }
        | Hata::PngSekmeliDesteklenmiyor { .. }
        | Hata::PngAnimasyonluDesteklenmiyor
        | Hata::PngFiltreTipiGecersiz { .. }
        | Hata::PngZlibBozuk { .. }
        | Hata::PngIdatYetersiz { .. }
        | Hata::PngIendYok
        | Hata::PngBoyutGecersiz { .. }
        | Hata::PngBlokCokBuyuk { .. }
        | Hata::PngSatirKisa { .. }
        | Hata::BilinmeyenBicim(_)
        | Hata::JpegBozuk { .. }
        | Hata::UzantiDesteklenmiyor { .. } => "okuma",
        Hata::JpegPikselKapsamDisi { .. } => "kodlama",
        Hata::BoyutGecersiz { .. }
        | Hata::KirpmaGecersiz { .. }
        | Hata::KaliteGecersiz(_)
        | Hata::HedefBoyutGecersiz(_)
        | Hata::HedefBulunamadi { .. } => "boyut",
        Hata::MetaVeriSinir { .. } => "meta",
        Hata::GirdiDosyaDegil(_)
        | Hata::CiktiCakismasi(_)
        | Hata::CiktiDiziniYok(_)
        | Hata::AyarGecersiz { .. }
        | Hata::RaporHatasi(_) => "kuyruk",
    }
}

/// Tüm çalışmanın özet raporu.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Rapor {
    /// Araç sürümü.
    pub surum: String,
    /// Kuru çalıştırma yapıldı mı?
    pub kuru_calistirma: bool,
    /// İşlenen dosya sayısı.
    pub toplam_dosya: usize,
    /// Başarılı dosya sayısı.
    pub basarili_dosya: usize,
    /// Başarısız dosya sayısı (atlanan bozuk dosyalar dâhil).
    pub basarisiz_dosya: usize,
    /// Dosya bazlı raporlar.
    pub dosyalar: Vec<DosyaRaporu>,
}

impl Rapor {
    /// Boş bir rapor oluşturur.
    #[must_use]
    pub fn yeni(kuru_calistirma: bool) -> Self {
        Self {
            surum: crate::SURUM.to_string(),
            kuru_calistirma,
            toplam_dosya: 0,
            basarili_dosya: 0,
            basarisiz_dosya: 0,
            dosyalar: Vec::new(),
        }
    }

    /// Bir dosya raporunu ekler ve sayaçları günceller.
    pub fn ekle(&mut self, dosya: DosyaRaporu) {
        if dosya.basarili {
            self.basarili_dosya += 1;
        } else {
            self.basarisiz_dosya += 1;
        }
        self.toplam_dosya += 1;
        self.dosyalar.push(dosya);
    }

    /// Raporu okunabilir metne çevirir.
    #[must_use]
    pub fn metin(&self) -> String {
        let mut satirlar = Vec::new();
        satirlar.push(format!(
            "PixelMill {} | kuru calistirma: {} | toplam: {} basarili: {} basarisiz: {}",
            self.surum,
            if self.kuru_calistirma {
                "evet"
            } else {
                "hayir"
            },
            self.toplam_dosya,
            self.basarili_dosya,
            self.basarisiz_dosya
        ));
        for d in &self.dosyalar {
            satirlar.push(d.ozet());
            for u in &d.uyarilar {
                satirlar.push(format!("     uyari: {u}"));
            }
        }
        satirlar.join("\n")
    }

    /// Raporu JSON olarak serileştirir.
    ///
    /// # Hatalar
    ///
    /// Serileştirme başarısız olursa `Hata::RaporHatasi` döner.
    pub fn json(&self) -> Result<String, Hata> {
        serde_json::to_string_pretty(self).map_err(|e| Hata::RaporHatasi(e.to_string()))
    }
}
