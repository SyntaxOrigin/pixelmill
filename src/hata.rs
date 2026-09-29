//! PixelMill hata turleri ve bunlarin `Display`/`Error` uygulamalari.
//!
//! Bu modul yalnizca hata *tanimi* icerir. `thiserror` veya benzeri bir turetici
//! crate kullanilmaz (WORKER_CONTRACT.md 4.3); `Display` elle yazilmistir.
//!
//! Tasarim ilkesi: kullanici girdisinden kaynaklanan hicbir durum `panic!` ile
//! sonlanmaz. Bozuk dosya, desteklenmeyen ozellik veya butce asimi gibi durumlar
//! `Result<T, Hata>` ile doner; boylece toplu is kuyrugunda tek bir hatali dosya
//! digerlerini durdurmaz.

use std::fmt;

/// PixelMill'in tum alt sistemlerinden donen hata turu.
///
/// Cagiran taraf `match` ile hangi sinifin (okuma, kodlama, boyut, kuyruk,
/// yazma) hatasi oldugunu ayirt edebilir.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Hata {
    /// Dosya sistemi hatasi (acma, okuma, yazma, yeniden adlandirma).
    Dosya {
        /// Uzerinde islem yapilan yol.
        yol: String,
        /// `std::io::Error` metninin ozeti.
        ayrinti: String,
    },
    /// Dosya imzasi taninmadi; PNG 8 baytlik imzasi ile baslamiyor.
    PngImzasiBozuk,
    /// PNG blok yapisi (uzunluk, tip, veri, CRC) hatali.
    PngBlokBozuk {
        /// Blok tipi metni, okunamadiysa `"?"`.
        blok_tipi: String,
        /// CRC veya uzunluk uyusmazliginin aciklamasi.
        ayrinti: String,
    },
    /// Blok CRC'si hesaplanan degerle eslesmiyor.
    PngCrcBozuk {
        /// CRC'nin ait oldugu blok tipi.
        blok_tipi: String,
        /// CRC alaninda yazan deger.
        dosyadan: u32,
        /// Uzerlenen veriden hesaplanan deger.
        hesaplanan: u32,
    },
    /// IHDR basligi eksik, kisa veya anlamsiz.
    PngBaslikBozuk {
        /// Baslikla ilgili ayrinti.
        ayrinti: String,
    },
    /// Renk tipi ile bit derinligi kombinasyonu PNG spec'te tanimsiz.
    PngRenkTipiGecersiz {
        /// Renk tipi kodu (0..=6).
        renk_tipi: u8,
        /// Bit derinligi.
        bit_derinligi: u8,
    },
    /// Interlace yontemi 1 (Adam7). Bu proje sekmeli PNG'yi **acikca reddeder**.
    PngSekmeliDesteklenmiyor {
        /// `interlace_method` alani (desteklenen tek deger 0'dir).
        yontem: u8,
    },
    /// Animasyonlu PNG (`acTL` chunk). Statik kare varsayilir.
    PngAnimasyonluDesteklenmiyor,
    /// Tarama satiri filtresi tipi 0..=4 disinda.
    PngFiltreTipiGecersiz {
        /// Ham filtre bayti.
        tip: u8,
    },
    /// zlib akisi bozuk veya inflate sirasinda hata olustu.
    PngZlibBozuk {
        /// inflate hatasinin ozeti.
        ayrinti: String,
    },
    /// IDAT verisi beklenen tum tarama satirlarini icermiyor.
    PngIdatYetersiz {
        /// Beklenen tarama satiri sayisi.
        beklenen: u64,
        /// Gercekten cozulen tarama satiri sayisi.
        bulunan: u64,
    },
    /// IEND isareti gorulmeden dosya sona erdi.
    PngIendYok,
    /// Girdi genisligi/yuksekligi sifir veya ustl siniri asiyor.
    PngBoyutGecersiz {
        /// Genislik (piksel).
        genislik: u64,
        /// Yukseklik (piksel).
        yukseklik: u64,
        /// Uygulanan ust sinirin aciklamasi.
        sebep: String,
    },
    /// PNG blok uzunlugu guvenlik sinirini asiyor ( bellek bombasi korumasi).
    PngBlokCokBuyuk {
        /// Blok tipi.
        blok_tipi: String,
        /// Bildirilen bayt sayisi.
        bayt: u64,
        /// Uygulanan ust sinir.
        sinir: u64,
    },
    /// Filtre uygulanacak tampon kaynak satirindan kisa.
    PngSatirKisa {
        /// Beklenen satir bayt sayisi.
        beklenen: usize,
        /// Elimine edilen bayt sayisi.
        bulunan: usize,
    },
    /// Yeniden boyutlandirma girdisi gecersiz (sifir boyut, olmayan filtre vb.).
    BoyutGecersiz {
        /// Hatanin aciklamasi.
        ayrinti: String,
    },
    /// Kirpma dikdortgeni kaynak cerceve disina tasiyor.
    KirpmaGecersiz {
        /// Kirpma dikdortgeni (x, y, genislik, yukseklik).
        dikdortgen: (u32, u32, u32, u32),
        /// Kaynak cerceve boyutu.
        kaynak: (u32, u32),
    },
    /// Kalite parametresi 0..=100 araliginda degil.
    KaliteGecersiz(u8),
    /// Hedef bayt sayisi sifir.
    HedefBoyutGecersiz(u64),
    /// Hedef boyuta hicbir kabul edilebilir ayarla ulasilamadi.
    HedefBulunamadi {
        /// Kullaniciya verilen hedef bayt.
        hedef: u64,
        /// Ulasilan en kucuk byte sayisi.
        en_iyi: u64,
    },
    /// Girdi yolu dosya degil.
    GirdiDosyaDegil(String),
    /// Dosya uzantisi desteklenmiyor.
    UzantiDesteklenmiyor {
        /// Dosya uzantisi (nokta haric, kucuk harf).
        uzanti: String,
        /// Kabul edilen uzantilar.
        desteklenen: String,
    },
    /// Cikti yolu bir kaynak dosyasiyla ayni; ustune yazma yapilmaz.
    CiktiCakismasi(String),
    /// Cikti dizini bulunamadi.
    CiktiDiziniYok(String),
    /// JPEG isareti tanindi ama piksel kodu kapsam disi.
    JpegPikselKapsamDisi {
        /// Islemin desteklenmedigi yeri.
        islem: String,
    },
    /// JPEG marker akisi bozuk veya desteklenmiyor.
    JpegBozuk {
        /// Hatanin aciklamasi.
        ayrinti: String,
    },
    /// Meta veri ayristirilirken boyut siniri asildi.
    MetaVeriSinir {
        /// Segmentin adi (ornegin `"tEXt"`).
        segment: String,
        /// Segmentin bayt uzunlugu.
        bayt: u64,
        /// Uygulanan ust sinir.
        sinir: u64,
    },
    /// Komut satirindan gelen deger aral disinda.
    AyarGecersiz {
        /// Ayarin adi.
        ad: String,
        /// Verilen deger.
        deger: String,
    },
    /// JSON rapor dosyasi yazilamadi veya serilestirilemedi.
    RaporHatasi(String),
    /// Bicim imzasi ne PNG ne JPEG.
    BilinmeyenBicim(String),
}

impl fmt::Display for Hata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Hata::Dosya { yol, ayrinti } => write!(f, "dosya islemi basarisiz ({yol}): {ayrinti}"),
            Hata::PngImzasiBozuk => write!(
                f,
                "PNG imzasi gecersiz: dosya 89 50 4E 47 0D 0A 1A 0A ile baslamali"
            ),
            Hata::PngBlokBozuk { blok_tipi, ayrinti } => {
                write!(f, "PNG blogu bozuk ({blok_tipi}): {ayrinti}")
            }
            Hata::PngCrcBozuk {
                blok_tipi,
                dosyadan,
                hesaplanan,
            } => write!(
                f,
                "PNG CRC hatasi ({blok_tipi}): dosyada {dosyadan:#010x}, hesaplanan {hesaplanan:#010x}"
            ),
            Hata::PngBaslikBozuk { ayrinti } => write!(f, "PNG IHDR basligi bozuk: {ayrinti}"),
            Hata::PngRenkTipiGecersiz {
                renk_tipi,
                bit_derinligi,
            } => write!(
                f,
                "gecersiz PNG renk tipi/bit derinligi kombinasyonu: renk tipi {renk_tipi}, bit derinligi {bit_derinligi}"
            ),
            Hata::PngSekmeliDesteklenmiyor { yontem } => write!(
                f,
                "sekmeli (Adam7) PNG desteklenmiyor: interlace_method={yontem}; \
                 PixelMill yalnizca interlace_method=0 okur, satirlari tek geciste isler"
            ),
            Hata::PngAnimasyonluDesteklenmiyor => write!(
                f,
                "animasyonlu PNG (acTL) desteklenmiyor: yalnizca tek kareli PNG islenir"
            ),
            Hata::PngFiltreTipiGecersiz { tip } => write!(
                f,
                "gecersiz PNG filtre tipi {tip}: yalnizca 0..=4 (None/Sub/Up/Average/Paeth) gecerli"
            ),
            Hata::PngZlibBozuk { ayrinti } => write!(f, "PNG zlib akisi cozulemedi: {ayrinti}"),
            Hata::PngIdatYetersiz {
                beklenen,
                bulunan,
            } => write!(
                f,
                "IDAT verisi yetersiz: {beklenen} satir beklenirken {bulunan} satir cozuldu"
            ),
            Hata::PngIendYok => write!(f, "PNG IEND isareti bulunamadi: dosya yarim kalmis"),
            Hata::PngBoyutGecersiz {
                genislik,
                yukseklik,
                sebep,
            } => write!(f, "PNG boyutu gecersiz ({genislik}x{yukseklik}): {sebep}"),
            Hata::PngBlokCokBuyuk {
                blok_tipi,
                bayt,
                sinir,
            } => write!(
                f,
                "PNG blogu guvenlik sinirini asiyor ({blok_tipi}): {bayt} bayt > {sinir} bayt"
            ),
            Hata::PngSatirKisa { beklenen, bulunan } => write!(
                f,
                "tarama satiri eksik: {beklenen} bayt bekleniyordu, {bulunan} bayt cozuldu"
            ),
            Hata::BoyutGecersiz { ayrinti } => write!(f, "yeniden boyutlandirma girdisi gecersiz: {ayrinti}"),
            Hata::KirpmaGecersiz {
                dikdortgen,
                kaynak,
            } => write!(
                f,
                "kirpma dikdortgeni kaynak disinda: x={}, y={}, g={}, y={} (kaynak {}x{})",
                dikdortgen.0, dikdortgen.1, dikdortgen.2, dikdortgen.3, kaynak.0, kaynak.1
            ),
            Hata::KaliteGecersiz(q) => write!(f, "kalite 0..=100 araliginda olmali, verilen: {q}"),
            Hata::HedefBoyutGecersiz(b) => write!(f, "hedef boyut sifirdan buyuk olmali, verilen: {b}"),
            Hata::HedefBulunamadi { hedef, en_iyi } => write!(
                f,
                "hedef boyuta ulasilamadi: {hedef} bayt istendi, en iyi sonuc {en_iyi} bayt"
            ),
            Hata::GirdiDosyaDegil(p) => write!(f, "girdi bir dosya degil: {p}"),
            Hata::UzantiDesteklenmiyor {
                uzanti,
                desteklenen,
            } => write!(f, "desteklenmeyen uzanti '.{uzanti}'; kabul edilenler: {desteklenen}"),
            Hata::CiktiCakismasi(p) => write!(
                f,
                "cikti yolu kaynak dosyayla ayni, ustune yazma yapilmaz: {p}"
            ),
            Hata::CiktiDiziniYok(p) => write!(f, "cikti dizini bulunamadi: {p}"),
            Hata::JpegPikselKapsamDisi { islem } => write!(
                f,
                "JPEG piksel isleme kapsam disi ({islem}): PixelMill JPEG'de yalnizca baslik ve \
                 kuantizasyon tablosunu okur, yeniden kodlama yapmaz"
            ),
            Hata::JpegBozuk { ayrinti } => write!(f, "JPEG akisi bozuk veya desteklenmiyor: {ayrinti}"),
            Hata::MetaVeriSinir { segment, bayt, sinir } => write!(
                f,
                "meta veri segmenti sinirini asiyor ({segment}): {bayt} bayt > {sinir} bayt"
            ),
            Hata::AyarGecersiz { ad, deger } => write!(f, "ayar gecersiz ({ad}): '{deger}'"),
            Hata::RaporHatasi(a) => write!(f, "rapor uretilemedi: {a}"),
            Hata::BilinmeyenBicim(p) => write!(
                f,
                "bilinmeyen bicim: {p}; yalnizca PNG (.png) ve JPEG (.jpg/.jpeg) taninir"
            ),
        }
    }
}

impl std::error::Error for Hata {}

/// `std::io::Error` uzerinden otomatik donusum icin yardimci: yolu hataya ekler.
pub fn io_hata(yol: &std::path::Path, kaynak: std::io::Error) -> Hata {
    Hata::Dosya {
        yol: yol.display().to_string(),
        ayrinti: kaynak.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_yardimci::ac;

    #[test]
    fn crc_hatasi_ayirt_edilebilir() {
        let hata = Hata::PngCrcBozuk {
            blok_tipi: "IDAT".to_string(),
            dosyadan: 0xdead_beef,
            hesaplanan: 0x0bad_f00d,
        };
        let metin = hata.to_string();
        if !metin.contains("IDAT") || !metin.contains("deadbeef") {
            panic!("Display beklenen bilgileri icermiyor: {metin}");
        }
    }

    #[test]
    fn interlacing_reddi_mesaji_spesifik() {
        let hata = Hata::PngSekmeliDesteklenmiyor { yontem: 1 };
        let metin = hata.to_string();
        if !metin.contains("Adam7") || !metin.contains("interlace_method=1") {
            panic!("interlace reddi belirtili olmali: {metin}");
        }
    }

    #[test]
    fn dosya_hatasi_yolu_yazar() {
        let hata = Hata::Dosya {
            yol: "C:\\girdi\\a.png".to_string(),
            ayrinti: "The system cannot find the file".to_string(),
        };
        if !hata.to_string().contains("girdi") {
            panic!("yol hatada gorunmeli");
        }
    }

    #[test]
    fn io_hata_yolu_ekler() {
        let kaynak = std::io::Error::new(std::io::ErrorKind::NotFound, "yok");
        let hata = io_hata(std::path::Path::new("a/b.png"), kaynak);
        match hata {
            Hata::Dosya { yol, ayrinti } => {
                if yol != "a/b.png" || !ayrinti.contains("yok") {
                    panic!("io_hata alanlari yanlis: {yol} / {ayrinti}");
                }
            }
            diger => panic!("beklenmeyen hata turu: {diger:?}"),
        }
    }

    #[test]
    fn hata_turleri_karsilastirilabilir() {
        // Toplu kuyrukta "bozuk dosya atlandi" kontrolu bu esitlige dayanir.
        if Hata::PngIendYok != Hata::PngIendYok {
            panic!("ayni hata turu esit olmali");
        }
        if Hata::PngIendYok == Hata::KaliteGecersiz(1) {
            panic!("farkli hata turleri farkli olmali");
        }
    }

    #[test]
    fn yardimci_basari_durumunu_gecirir() {
        let sayi: i32 = ac(Ok::<i32, Hata>(7));
        if sayi != 7 {
            panic!("yardimci degeri degistirmemeli");
        }
    }
}
