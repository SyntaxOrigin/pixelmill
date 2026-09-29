//! JPEG **başlık ve kuantizasyon tablosu** okuyucu (baseline).
//!
//! ## Kapsam
//!
//! Bu modül JPEG dosyasının **başlık bölümünü** okur:
//!
//! - `SOI` / `EOI` imzaları,
//! - `APP0..APP15` (JFIF, EXIF), `COM` yorum segmentleri,
//! - `DQT` kuantizasyon tabloları (zig-zag düzeninden doğal düzene çevrilir),
//! - `DHT` Huffman tablosu tanımlarının varlığı (tablonun kendisi saklanmaz),
//! - `SOF0` (baseline) / `SOF1` (extended sequential) çerçeve başlığı: boyut,
//!   bileşen sayısı ve başına bileşen örnekleme faktörleri,
//! - `SOS` tarama başlığı (bileşen seçimi, Huffman tablo referansları).
//!
//! Tarama verisi entropy-kodludur ve **çözülmez**; `SOS` görülür görülmez
//! okuma durur. Bu nedenle piksel yeniden boyutlandırma veya yeniden kodlama
//! JPEG üzerinde **yapılamaz**; ilgili denemeler `Hata::JpegPikselKapsamDisi`
//! ile açıkça reddedilir (MANIFEST kart 04, madde 1 ve "Ertelenen").
//!
//! ## Dayanak
//!
//! ITU T.81 (JPEG) ve JFIF 1.02. `SOF2` (progressive) okunabilir ama
//! yeniden kodlama kapsam dışı olduğu için açıkça belirtilir.

use std::io::Read;
use std::path::Path;

use crate::hata::{io_hata, Hata};
use crate::meta::exif_etiketleri;

/// JPEG başlangıç imzası (`SOI` = `FF D8`).
pub const JPEG_IMZASI: [u8; 2] = [0xFF, 0xD8];

/// Bir kuantizasyon tablosunun 64 girişi (doğal düzende).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KuantizasyonTablosu {
    /// Tablo numarası (0..=3).
    pub kimlik: u8,
    /// Tablonun hassasiyeti (8 veya 16 bit).
    hassasiyet: u8,
    /// 64 çarpan, doğal düzende (`DQT` içinde zig-zag sırasındadır).
    pub degerler: [u16; 64],
}

impl KuantizasyonTablosu {
    /// Tablonun hassasiyetini (bit) döndürür.
    #[must_use]
    pub fn hassasiyet(&self) -> u8 {
        self.hassasiyet
    }

    /// Doğal sıradaki ilk çarpan (DC katsayısı çarpanı).
    #[must_use]
    pub fn dc_carpan(&self) -> u16 {
        self.degerler[0]
    }
}

/// JPEG'in çerçeve (frame) başlığındaki tek bir bileşen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bilesen {
    /// Bileşen tanımlayıcısı (1 = Y, 2 = Cb, 3 = Cr, 4 = Y'CbCr' vb.).
    pub kimlik: u8,
    /// Yatay örnekleme faktörü.
    pub yatay: u8,
    /// Dikey örnekleme faktörü.
    pub dikey: u8,
    /// Bu bileşenin kullandığı kuantizasyon tablosu numarası.
    pub kuantizasyon: u8,
}

impl Bilesen {
    /// Bileşenin yatay/dikey örnekleme çarpanı (`yatay * dikey`).
    ///
    /// JPEG'de bu değer, `4:2:0` için 2, `4:4:4` için 1'dir.
    #[must_use]
    pub fn ornek_carpani(&self) -> u8 {
        self.yatay * self.dikey
    }
}

/// `SOF0` / `SOF1` çerçeve başlığının çözülmüş hali.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cerceve {
    /// Görüntü genişliği (piksel).
    pub genislik: u32,
    /// Görüntü yüksekliği (piksel).
    pub yukseklik: u32,
    /// Bileşen sayısı.
    pub bilesen_sayisi: u8,
    /// Bileşen tanımları.
    pub bilesenler: Vec<Bilesen>,
    /// `SOF` işaretçisi (`C0` baseline, `C1` extended sequential).
    pub isaretci: u8,
}

impl Cerceve {
    /// Ana (en yüksek örnekleme çarpanına sahip) bileşenin örnekleme çarpanı.
    ///
    /// `4:2:0` için 2 döner. Renk alt örneklemesi bilgisi buradan okunur.
    #[must_use]
    pub fn en_yuksek_carp(&self) -> u8 {
        self.bilesenler
            .iter()
            .map(Bilesen::ornek_carpani)
            .max()
            .unwrap_or(1)
    }
}

/// Okunmuş JPEG başlığı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpegBaslik {
    /// Çerçeve bilgisi (`SOF0`/`SOF1` bulunduysa).
    pub cerceve: Option<Cerceve>,
    /// Bulunan kuantizasyon tabloları.
    pub kuantizasyon: Vec<KuantizasyonTablosu>,
    /// `DHT` (Huffman tablosu) tanımı sayısı.
    pub huffman_tablo_sayisi: u16,
    /// `APP` segmentlerinin uygulama etiketleri (0..=15).
    pub app_etiketleri: Vec<u8>,
    /// `APP1`/`Exif` verisi bulunduysa TIFF/EXIF gövdesi.
    pub exif: Option<Vec<u8>>,
    /// `JFIF` sürümü bulunduysa sürüm numarası.
    pub jfif_surumu: Option<u8>,
    /// `JFIF` yoğunluk birimi (`0` = yok, `1` = inç, `2` = cm).
    pub jfif_birim: Option<u8>,
    /// Dosyada `SOS` (tarama başlangıcı) görüldü mü?
    pub tarama_basladi: bool,
    /// Görülen marker türlerinin okunabilir adları.
    pub markerlar: Vec<String>,
}

impl JpegBaslik {
    /// Kuantizasyon tablosunu kimlik ile getirir.
    #[must_use]
    pub fn tablo(&self, kimlik: u8) -> Option<&KuantizasyonTablosu> {
        self.kuantizasyon.iter().find(|t| t.kimlik == kimlik)
    }

    /// EXIF alanlarını okur (varsa).
    ///
    /// # Hatalar
    ///
    /// EXIF gövdesi bozuksa hata döner.
    pub fn exif_alanlari(&self) -> Result<Vec<crate::meta::ExifAlani>, Hata> {
        match &self.exif {
            Some(tiff) => exif_etiketleri(tiff),
            None => Ok(Vec::new()),
        }
    }

    /// Bu JPEG'in piksel olarak yeniden boyutlandırılıp kodlanabileceğini bildirir.
    ///
    /// PixelMill JPEG'i yeniden kodlamaz; bu metot çağıran tarafa bilinçli bir
    /// hata döndürmek için kullanılır.
    ///
    /// # Hatalar
    ///
    /// Her çağrıda `Hata::JpegPikselKapsamDisi` döner.
    pub fn piksel_kodlama_desteklenmiyor(islem: &str) -> Hata {
        Hata::JpegPikselKapsamDisi {
            islem: islem.to_string(),
        }
    }
}

/// JPEG zig-zag → doğal düzen dönüşüm tablosu (64 giriş).
///
/// ITU T.81, Tablo B.1: kuantizasyon katsayıları zig-zag sırasında saklanır.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// `DQT` verisini kuantizasyon tablosuna çevirir.
///
/// # Hatalar
///
/// Veri 65 bayttan kısa veya tablo kimliği 0..=3 dışındaysa hata döner.
pub fn dqt_ayir(kimlik_bilgisi: u8, veri: &[u8]) -> Result<KuantizasyonTablosu, Hata> {
    let kimlik = kimlik_bilgisi & 0x0F;
    let hassasiyet = (kimlik_bilgisi >> 4) & 0x0F;
    if kimlik > 3 {
        return Err(Hata::JpegBozuk {
            ayrinti: format!("DQT tablo kimligi 0..=3 olmali, verilen {kimlik}"),
        });
    }
    let beklenen = if hassasiyet == 0 { 65 } else { 129 };
    if veri.len() != beklenen {
        return Err(Hata::JpegBozuk {
            ayrinti: format!(
                "DQT verisi {beklenen} bayt olmali ({hassasiyet} bit), verilen {}",
                veri.len()
            ),
        });
    }
    let mut degerler = [0u16; 64];
    for (zigzag_indeks, dogal_indeks) in ZIGZAG.iter().enumerate() {
        let bayt = if hassasiyet == 0 {
            u16::from(veri[1 + zigzag_indeks])
        } else {
            u16::from_be_bytes([veri[1 + 2 * zigzag_indeks], veri[2 + 2 * zigzag_indeks]])
        };
        degerler[*dogal_indeks] = bayt;
    }
    Ok(KuantizasyonTablosu {
        kimlik,
        hassasiyet,
        degerler,
    })
}

/// Bir okuyucudan JPEG başlığını okur.
///
/// # Hatalar
///
/// Dosya JPEG değilse, marker akışı bozuksa veya segment sınırları aşılırsa
/// uygun `Hata` döner.
pub fn jpeg_baslik_oku<R: Read>(kaynak: R) -> Result<JpegBaslik, Hata> {
    let mut okuyucu = Okuyucu::yeni(kaynak)?;
    okuyucu.baslik_oku()
}

/// JPEG dosyasını açar ve başlığını okur.
///
/// # Hatalar
///
/// Dosya açılamazsa veya JPEG değilse hata döner.
pub fn jpeg_dosya_basligi(yol: &Path) -> Result<JpegBaslik, Hata> {
    let dosya = std::fs::File::open(yol).map_err(|e| io_hata(yol, e))?;
    jpeg_baslik_oku(std::io::BufReader::with_capacity(64 * 1024, dosya))
}

/// Marker akışını gezen iç okuyucu.
struct Okuyucu<R: Read> {
    kaynak: R,
    baslik: JpegBaslik,
    /// Ham tarama verisi görüldükten sonra `true` olur; okuma durur.
    tarama_basladi: bool,
}

impl<R: Read> Okuyucu<R> {
    /// Yeni okuyucu oluşturur ve `SOI` imzasını doğrular.
    fn yeni(kaynak: R) -> Result<Self, Hata> {
        let mut okuyucu = Okuyucu {
            kaynak,
            baslik: JpegBaslik {
                cerceve: None,
                kuantizasyon: Vec::new(),
                huffman_tablo_sayisi: 0,
                app_etiketleri: Vec::new(),
                exif: None,
                jfif_surumu: None,
                jfif_birim: None,
                tarama_basladi: false,
                markerlar: Vec::new(),
            },
            tarama_basladi: false,
        };
        let soi = okuyucu.baytlar_oku(2)?;
        if soi != JPEG_IMZASI {
            return Err(Hata::BilinmeyenBicim(
                soi.iter().map(|&b| format!("{b:#04x}")).collect::<String>(),
            ));
        }
        Ok(okuyucu)
    }

    /// Tek bayt okur.
    fn bayt_oku(&mut self) -> Result<u8, Hata> {
        let mut tampon = [0u8; 1];
        self.kaynak
            .read_exact(&mut tampon)
            .map_err(|e| Hata::JpegBozuk {
                ayrinti: format!("beklenmeyen dosya sonu: {e}"),
            })?;
        Ok(tampon[0])
    }

    /// Tam sayı bayt okur.
    fn baytlar_oku(&mut self, adet: usize) -> Result<Vec<u8>, Hata> {
        if adet == 0 {
            return Ok(Vec::new());
        }
        let mut tampon = vec![0u8; adet];
        self.kaynak
            .read_exact(&mut tampon)
            .map_err(|e| Hata::JpegBozuk {
                ayrinti: format!("beklenmeyen dosya sonu ({adet} bayt isteniyordu): {e}"),
            })?;
        Ok(tampon)
    }

    /// `0xFF` dolgularını atlayarak sonraki marker'ı bulur.
    fn marker_bekle(&mut self) -> Result<u8, Hata> {
        let ilk = self.bayt_oku()?;
        if ilk != 0xFF {
            return Err(Hata::JpegBozuk {
                ayrinti: format!("marker 0xFF ile baslamali, bulunan {ilk:#04x}"),
            });
        }
        let mut bayt = self.bayt_oku()?;
        // `0xFF 0xFF` dolgu baytları atlanır.
        while bayt == 0xFF {
            bayt = self.bayt_oku()?;
        }
        Ok(bayt)
    }

    /// Başlık bölümünün tamamını okur (`SOS` veya `EOI` görülene dek).
    fn baslik_oku(&mut self) -> Result<JpegBaslik, Hata> {
        loop {
            let marker = self.marker_bekle()?;
            let ad = marker_adi(marker);
            self.baslik.markerlar.push(ad);
            match marker {
                0xD8 | 0x01 | 0xD0..=0xD7 => continue,
                0xD9 => break,
                0xDA => {
                    // SOS: tarama verisi entropy-kodludur, burada durulur.
                    let veri = self.uzunluklu_segment(marker)?;
                    self.baslik.tarama_basladi = true;
                    self.tarama_basladi = true;
                    let _ = veri;
                    break;
                }
                0xC0 | 0xC1 => {
                    let veri = self.uzunluklu_segment(marker)?;
                    self.baslik.cerceve = Some(cerceve_ayir(marker, &veri)?);
                }
                0xC2 => {
                    // Progressive: başlık okunabilir ama yeniden kodlama yok.
                    let veri = self.uzunluklu_segment(marker)?;
                    self.baslik.cerceve = Some(cerceve_ayir(marker, &veri)?);
                    break;
                }
                0xC4 => {
                    let veri = self.uzunluklu_segment(marker)?;
                    self.baslik.huffman_tablo_sayisi = huffman_sayi(&veri);
                }
                0xDB => {
                    let veri = self.uzunluklu_segment(marker)?;
                    self.dqt_isle(&veri)?;
                }
                0xE0..=0xEF => {
                    let veri = self.uzunluklu_segment(marker)?;
                    let etiket = marker - 0xE0;
                    self.baslik.app_etiketleri.push(etiket);
                    if etiket == 0 {
                        self.jfif_ayir(&veri);
                    } else if etiket == 1
                        && veri.starts_with(b"Exif\0\0")
                        && self.baslik.exif.is_none()
                    {
                        self.baslik.exif = Some(veri[6..].to_vec());
                    }
                }
                0xFE => {
                    let _ = self.uzunluklu_segment(marker)?;
                }
                _ => {
                    let _ = self.uzunluklu_segment(marker)?;
                }
            }
        }
        Ok(self.baslik.clone())
    }

    /// Uzunluğu segmentin ilk iki baytından okuyup gövdeyi döndürür.
    fn uzunluklu_segment(&mut self, marker: u8) -> Result<Vec<u8>, Hata> {
        let uzunluk_baytlari = self.baytlar_oku(2)?;
        let ham = u16::from_be_bytes([uzunluk_baytlari[0], uzunluk_baytlari[1]]);
        if u32::from(ham) < 2 {
            return Err(Hata::JpegBozuk {
                ayrinti: format!("marker {marker:#04x} segment uzunlugu 2'den kucuk: {ham}"),
            });
        }
        self.baytlar_oku(usize::from(ham) - 2)
    }

    /// `DQT` segment gövdesini (bir veya birden çok tablo) işler.
    fn dqt_isle(&mut self, veri: &[u8]) -> Result<(), Hata> {
        let mut konum = 0usize;
        while konum < veri.len() {
            let bilgi = veri[konum];
            let hassasiyet = (bilgi >> 4) & 0x0F;
            if hassasiyet > 1 {
                return Err(Hata::JpegBozuk {
                    ayrinti: format!("DQT hassasiyeti 0 veya 1 olmali, verilen {hassasiyet}"),
                });
            }
            let uzunluk = if hassasiyet == 0 { 65 } else { 129 };
            if konum + uzunluk > veri.len() {
                return Err(Hata::JpegBozuk {
                    ayrinti: "DQT tablosu segment sonuna tasiyor".to_string(),
                });
            }
            let tablo = dqt_ayir(bilgi, &veri[konum..konum + uzunluk])?;
            self.baslik.kuantizasyon.push(tablo);
            konum += uzunluk;
        }
        Ok(())
    }

    /// `APP0` JFIF segmentini ayrıştırır.
    fn jfif_ayir(&mut self, veri: &[u8]) {
        if veri.len() >= 14 && veri.starts_with(b"JFIF\0") {
            self.baslik.jfif_surumu = Some(veri[5]);
            self.baslik.jfif_birim = Some(veri[7]);
        }
    }
}

/// `DHT` gövdesindeki tablo sayısını hesaplar (`Tc/Th` baytları).
fn huffman_sayi(veri: &[u8]) -> u16 {
    let mut sayi = 0u16;
    let mut konum = 0usize;
    while konum < veri.len() {
        let hassasiyet = (veri[konum] >> 4) & 0x0F;
        let tablo_bayt = if hassasiyet == 0 { 17 } else { 33 };
        if konum + tablo_bayt > veri.len() {
            break;
        }
        konum += tablo_bayt;
        sayi = sayi.saturating_add(1);
    }
    sayi
}

/// `SOF0`/`SOF1` gövdesini çerçeve bilgisine çevirir.
fn cerceve_ayir(marker: u8, veri: &[u8]) -> Result<Cerceve, Hata> {
    if veri.len() < 6 {
        return Err(Hata::JpegBozuk {
            ayrinti: "SOF gövdesi çok kısa".to_string(),
        });
    }
    let _hassasiyet = veri[0];
    let yukseklik = u32::from(u16::from_be_bytes([veri[1], veri[2]]));
    let genislik = u32::from(u16::from_be_bytes([veri[3], veri[4]]));
    let bilesen_sayisi = veri[5];
    if bilesen_sayisi == 0 || usize::from(bilesen_sayisi) * 3 + 6 > veri.len() {
        return Err(Hata::JpegBozuk {
            ayrinti: format!("SOF bileşen sayısı geçersiz: {bilesen_sayisi}"),
        });
    }
    let mut bilesenler = Vec::with_capacity(usize::from(bilesen_sayisi));
    for i in 0..usize::from(bilesen_sayisi) {
        let o = 6 + i * 3;
        bilesenler.push(Bilesen {
            kimlik: veri[o],
            yatay: veri[o + 1] >> 4,
            dikey: veri[o + 1] & 0x0F,
            kuantizasyon: veri[o + 2],
        });
    }
    Ok(Cerceve {
        genislik,
        yukseklik,
        bilesen_sayisi,
        bilesenler,
        isaretci: marker,
    })
}

/// Marker kodu için okunabilir ad.
#[must_use]
pub fn marker_adi(marker: u8) -> String {
    match marker {
        0xD8 => "SOI".to_string(),
        0xD9 => "EOI".to_string(),
        0xDA => "SOS".to_string(),
        0xC0 => "SOF0".to_string(),
        0xC1 => "SOF1".to_string(),
        0xC2 => "SOF2".to_string(),
        0xC4 => "DHT".to_string(),
        0xDB => "DQT".to_string(),
        0xFE => "COM".to_string(),
        0xE0..=0xEF => format!("APP{}", marker - 0xE0),
        0xD0..=0xD7 => format!("RST{}", marker - 0xD0),
        _ => format!("MARKER_{marker:#04x}"),
    }
}
