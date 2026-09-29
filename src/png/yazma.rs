//! PNG yazıcı: satır akışıyla, filtreli ve DEFLATE sıkıştırılmış çıktı üretir.
//!
//! Yazma akışı PNG spec'e (RFC 2083) göre şöyledir:
//!
//! ```text
//! imza (8) | IHDR | [PLTE] | [tRNS] | [tEXt ...] | IDAT (bir veya birçok) | IEND
//! ```
//!
//! - Her `IDAT` bloğu en fazla `IDAT_PARCA` bayt taşır; taşan veri bir sonraki
//!   bloğa taşılır (PNG, `IDAT` bloğunun tek parça olmasını zorunlu kılmaz).
//! - Her satır, beş filtre tipi arasından PNG spec'in önerdiği
//!   **en küçük mutlak-toplam sapma** sezgiseliyle seçilir
//!   (bkz. [`crate::png::filtre::en_iyi_filtre`]).
//! - Sıkıştırma `flate2` (`miniz_oxide`, saf Rust) ile yapılır; C kütüphanesi
//!   bağlantısı yoktur.
//!
//! Bellek kullanimı: iki satır tamponu + `IDAT` parça tamponu + `flate2`'nin
//! kendi çalışma tamponu. Görüntünün tamamı bellekte tutulmaz.

use std::io::Write;

use crate::hata::Hata;
use crate::meta::{anahtari_dogrula, MetinKaydi};
use crate::png::blok::{crc32_tip_ile, PNG_IMZASI};
use crate::png::filtre::{en_iyi_filtre, filtrele};

/// Tek bir `IDAT` blogunun en fazla tasidigi veri boyutu (bayt).
///
/// PNG, veriyi bir veya birçok `IDAT` bloguna bölebilir. 64 KiB, gercekci
/// kodlayicilarin kullandigi degerdir ve zlib cikti tamponuyla ayni olcektir.
pub const IDAT_PARCA: usize = 64 * 1024;

/// PNG çıktısının biçim tanımları.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngSecenek {
    /// Görüntü genişliği (piksel).
    pub genislik: u32,
    /// Görüntü yüksekliği (piksel).
    pub yukseklik: u32,
    /// Örnek derinliği (1, 2, 4, 8 veya 16).
    pub bit_derinligi: u8,
    /// Renk tipi (0, 2, 3, 4, 6).
    pub renk_tipi: u8,
    /// `PLTE` paleti (yalnız renk tipi 3 için doludur).
    pub palet: Vec<[u8; 3]>,
    /// Yazılacak `tEXt` kayıtları (anahtar geçerli olmalıdır).
    pub metin: Vec<MetinKaydi>,
    /// `tRNS` şeffaflık anahtarı / palet alfaları (varsa).
    pub trns: Option<Vec<u8>>,
}

impl PngSecenek {
    /// Verilen biçim tanımlarıyla yeni bir seçenek kümesi oluşturur.
    ///
    /// # Hatalar
    ///
    /// Boyut sıfır veya sınırları aşıyorsa, renk tipi geçersizse, palet
    /// eksik/çok büyükse veya metin anahtarı geçersizse hata döner.
    pub fn yeni(
        genislik: u32,
        yukseklik: u32,
        bit_derinligi: u8,
        renk_tipi: u8,
    ) -> Result<Self, Hata> {
        let secenek = Self {
            genislik,
            yukseklik,
            bit_derinligi,
            renk_tipi,
            palet: Vec::new(),
            metin: Vec::new(),
            trns: None,
        };
        let baslik = crate::png::okuma::Baslik {
            genislik,
            yukseklik,
            bit_derinligi,
            renk_tipi,
            sikistirma: 0,
            filtre_yontemi: 0,
            sekme: 0,
        };
        crate::png::okuma::basligi_dogrula(&baslik)?;
        if renk_tipi == 3 {
            return Err(Hata::PngBaslikBozuk {
                ayrinti: "renk tipi 3 icin palet zorunludur".to_string(),
            });
        }
        Ok(secenek)
    }

    /// Paletli çıktı için seçenek kümesi oluşturur.
    ///
    /// # Hatalar
    ///
    /// Palet boş veya 256 girişten fazlaysa hata döner; `bit_derinligi` palet
    /// büyüklüğüyle tutarsızsa hata döner.
    pub fn paletli(genislik: u32, yukseklik: u32, palet: Vec<[u8; 3]>) -> Result<Self, Hata> {
        if palet.is_empty() || palet.len() > 256 {
            return Err(Hata::PngBaslikBozuk {
                ayrinti: format!("palet 1..=256 giris olmali, verilen {}", palet.len()),
            });
        }
        let bit_derinligi = if palet.len() <= 2 {
            1
        } else if palet.len() <= 4 {
            2
        } else if palet.len() <= 16 {
            4
        } else {
            8
        };
        let secenek = Self {
            genislik,
            yukseklik,
            bit_derinligi,
            renk_tipi: 3,
            palet,
            metin: Vec::new(),
            trns: None,
        };
        crate::png::okuma::basligi_dogrula(&crate::png::okuma::Baslik {
            genislik,
            yukseklik,
            bit_derinligi,
            renk_tipi: 3,
            sikistirma: 0,
            filtre_yontemi: 0,
            sekme: 0,
        })?;
        Ok(secenek)
    }

    /// Filtrelemede kullanılacak piksel başına bayt sayısı.
    #[must_use]
    pub fn piksel_bayt(&self) -> usize {
        let kanal = match self.renk_tipi {
            0 | 3 => 1u32,
            2 => 3,
            4 => 2,
            _ => 4,
        };
        ((u32::from(self.bit_derinligi) * kanal) / 8).max(1) as usize
    }

    /// Filtrelenmemiş bir satırın bayt sayısı.
    #[must_use]
    pub fn satir_bayt(&self) -> usize {
        let kanal = match self.renk_tipi {
            0 | 3 => 1u32,
            2 => 3,
            4 => 2,
            _ => 4,
        };
        let bit = u64::from(self.bit_derinligi) * u64::from(kanal) * u64::from(self.genislik);
        bit.div_ceil(8) as usize
    }
}

/// Sıkıştırılmış `IDAT` verisini biriktiren tampon.
///
/// PNG'de `IDAT` bloğunun uzunluğu ve CRC'si **sıkıştırılmış** veriyi kapsar.
/// Sıkıştırıcı doğrudan dosyaya yazarsa bu uzunluk/CRC'yi önceden bilmek
/// imkânsızdır. Bu yüzden sıkıştırıcı önce bu tampona yazar; dolduğunda
/// alınan baytlar `IDAT` bloğu olarak **dosya düzeyinde** çerçevelenir.
#[derive(Debug, Default)]
struct Biriktirici {
    veri: Vec<u8>,
}

impl Biriktirici {
    /// Yeni, boş bir biriktirici oluşturur.
    fn yeni() -> Self {
        Self {
            veri: Vec::with_capacity(IDAT_PARCA * 2),
        }
    }
}

impl Write for Biriktirici {
    fn write(&mut self, tampon: &[u8]) -> std::io::Result<usize> {
        self.veri.extend_from_slice(tampon);
        Ok(tampon.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// PNG imzası, `IHDR` ve yardımcı blokları yazıp `IDAT` akışını açar.
pub struct PngYazici<W: Write> {
    kodlayici: flate2::write::ZlibEncoder<Biriktirici>,
    cikti: W,
    satir_bayt: usize,
    bpp: usize,
    beklenen_yukseklik: u32,
    yazilan_satir: u32,
    onceki: Vec<u8>,
    filtreli: Vec<u8>,
}

impl<W: Write> PngYazici<W> {
    /// Yeni bir yazıcı açar ve imza + `IHDR` + yardımcı blokları yazar.
    ///
    /// # Hatalar
    ///
    /// Seçenek tutarsızsa veya ilk yazma hata döndürürse hata döner.
    pub fn yeni(cikti: W, secenek: PngSecenek) -> Result<Self, Hata> {
        let satir_bayt = secenek.satir_bayt();
        let bpp = secenek.piksel_bayt();
        let mut kok = Vec::new();
        kok.extend_from_slice(&PNG_IMZASI);

        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&secenek.genislik.to_be_bytes());
        ihdr.extend_from_slice(&secenek.yukseklik.to_be_bytes());
        ihdr.push(secenek.bit_derinligi);
        ihdr.push(secenek.renk_tipi);
        ihdr.extend_from_slice(&[0, 0, 0]);
        blog_yaz(&mut kok, b"IHDR", &ihdr)?;

        if secenek.renk_tipi == 3 {
            let mut plte = Vec::with_capacity(secenek.palet.len() * 3);
            for giriş in &secenek.palet {
                plte.extend_from_slice(giriş);
            }
            blog_yaz(&mut kok, b"PLTE", &plte)?;
        }
        if let Some(trns) = &secenek.trns {
            blog_yaz(&mut kok, b"tRNS", trns)?;
        }
        for kayit in &secenek.metin {
            anahtari_dogrula(&kayit.anahtar)?;
            let mut veri = Vec::new();
            veri.extend_from_slice(kayit.anahtar.as_bytes());
            veri.push(0);
            veri.extend(kayit.deger.chars().map(|c| c as u8));
            blog_yaz(&mut kok, b"tEXt", &veri)?;
        }

        let mut cikti = cikti;
        cikti.write_all(&kok).map_err(|e| Hata::Dosya {
            yol: "<cikti>".to_string(),
            ayrinti: e.to_string(),
        })?;

        let kodlayici =
            flate2::write::ZlibEncoder::new(Biriktirici::yeni(), flate2::Compression::default());
        Ok(Self {
            kodlayici,
            cikti,
            satir_bayt,
            bpp,
            beklenen_yukseklik: secenek.yukseklik,
            yazilan_satir: 0,
            onceki: vec![0u8; satir_bayt],
            filtreli: vec![0u8; satir_bayt],
        })
    }

    /// Filtrelenmemiş bir satırı yazar.
    ///
    /// Satır, [`PngSecenek::satir_bayt`] bayt olmalıdır; paletli çıktıda bu
    /// baytlar **paketlenmiş indislerdir** (bkz. [`crate::kodlama`]).
    ///
    /// # Hatalar
    ///
    /// Satır uzunluğu yanlışsa, beklenenden fazla satır yazılıyorsa veya
    /// sıkıştırma/yazma hata döndürürse hata döner.
    pub fn yaz(&mut self, satir: &[u8]) -> Result<(), Hata> {
        if satir.len() != self.satir_bayt {
            return Err(Hata::PngSatirKisa {
                beklenen: self.satir_bayt,
                bulunan: satir.len(),
            });
        }
        if self.yazilan_satir >= self.beklenen_yukseklik {
            return Err(Hata::PngBaslikBozuk {
                ayrinti: format!("{} satirdan fazla yazildi", self.beklenen_yukseklik),
            });
        }
        let onceki = if self.yazilan_satir == 0 {
            None
        } else {
            Some(self.onceki.as_slice())
        };
        let tip = en_iyi_filtre(self.bpp, onceki, satir);
        filtrele(tip, self.bpp, onceki, satir, &mut self.filtreli)?;
        self.kodlayici.write_all(&[tip]).map_err(zlib_hata)?;
        self.kodlayici
            .write_all(&self.filtreli)
            .map_err(zlib_hata)?;
        self.onceki.copy_from_slice(satir);
        self.yazilan_satir += 1;
        if self.kodlayici.get_ref().veri.len() >= IDAT_PARCA {
            self.idat_temizle()?;
        }
        Ok(())
    }

    /// Birikmiş sıkıştırılmış veriyi `IDAT` bloğu olarak **dosyaya** yazar.
    ///
    /// Blok çerçevesi (`uzunluk + "IDAT" + veri + CRC`) sıkıştırılmış akışın
    /// dışında, dosya düzeyinde yazılır; böylece PNG okuyucuları bloğu
    /// doğru çerçevelenmiş olarak görür.
    fn idat_temizle(&mut self) -> Result<(), Hata> {
        let veri = std::mem::take(&mut self.kodlayici.get_mut().veri);
        self.kodlayici.get_mut().veri = Vec::with_capacity(IDAT_PARCA * 2);
        if veri.is_empty() {
            return Ok(());
        }
        let crc = crc32_tip_ile(b"IDAT", &veri);
        let mut blok = Vec::with_capacity(veri.len() + 12);
        blok.extend_from_slice(&(veri.len() as u32).to_be_bytes());
        blok.extend_from_slice(b"IDAT");
        blok.extend_from_slice(&veri);
        blok.extend_from_slice(&crc.to_be_bytes());
        self.cikti.write_all(&blok).map_err(|e| Hata::Dosya {
            yol: "<cikti>".to_string(),
            ayrinti: e.to_string(),
        })
    }

    /// Akışı kapatır, `IEND` yazar ve alttaki yazıcıyı geri döndürür.
    ///
    /// # Hatalar
    ///
    /// Eksik satır yazıldıysa veya sıkıştırma/yazma hata döndürürse hata döner.
    pub fn bitir(mut self) -> Result<W, Hata> {
        if self.yazilan_satir != self.beklenen_yukseklik {
            return Err(Hata::PngBaslikBozuk {
                ayrinti: format!(
                    "{} satirdan {} tanesi yazildi",
                    self.beklenen_yukseklik, self.yazilan_satir
                ),
            });
        }
        // zlib akisini kapat ve kalan sikistirilmis baytlari bosalt.
        let kalan = self.kodlayici.finish().map_err(|e| Hata::PngZlibBozuk {
            ayrinti: e.to_string(),
        })?;
        self.kodlayici =
            flate2::write::ZlibEncoder::new(Biriktirici::yeni(), flate2::Compression::default());
        self.kodlayici.get_mut().veri = kalan.veri;
        self.idat_temizle()?;

        let mut blok = Vec::with_capacity(12);
        blok.extend_from_slice(&0u32.to_be_bytes());
        blok.extend_from_slice(b"IEND");
        blok.extend_from_slice(&crc32_tip_ile(b"IEND", &[]).to_be_bytes());
        self.cikti.write_all(&blok).map_err(|e| Hata::Dosya {
            yol: "<cikti>".to_string(),
            ayrinti: e.to_string(),
        })?;
        Ok(self.cikti)
    }
}

/// Sıkıştırma katmanından gelen `std::io::Error` değerini `Hata`'ya çevirir.
fn zlib_hata(hata: std::io::Error) -> Hata {
    Hata::PngZlibBozuk {
        ayrinti: hata.to_string(),
    }
}

/// Tek bir PNG bloğunu (uzunluk + tip + veri + CRC) bir tampona yazar.
///
/// # Hatalar
///
/// Yalnızca `Vec` aritmetiği nedeniyle hata döndürmez; dönüş tipi
/// `PngYazici` ile uyum için `Result` tutulur.
fn blog_yaz(kok: &mut Vec<u8>, tip: &[u8; 4], veri: &[u8]) -> Result<(), Hata> {
    let crc = crc32_tip_ile(tip, veri);
    kok.extend_from_slice(&(veri.len() as u32).to_be_bytes());
    kok.extend_from_slice(tip);
    kok.extend_from_slice(veri);
    kok.extend_from_slice(&crc.to_be_bytes());
    Ok(())
}

/// Bir satır üretici ile PNG çıktısını vektöre kodlar.
///
/// Akış hâlinde yazmanın basit bir sarmalayıcısıdır: her satır ayrı ayrı
/// filtrelenip sıkıştırılır, ancak *sonuç* tek bir `Vec<u8>` olarak döner.
/// Büyük görüntülerde bellek tasarrufu için [`PngYazici`] doğrudan dosyaya
/// yazılmalıdır.
///
/// `satir_uretici`, verilen satır numarası için filtrelenmemiş satır baytlarını
/// üretir.
///
/// # Hatalar
///
/// Satır sayısı yükseklikle uyuşmazsa veya yazma hata döndürürse hata döner.
pub fn png_kodla(
    secenek: &PngSecenek,
    satir_uretici: &mut dyn FnMut(u32) -> Vec<u8>,
) -> Result<Vec<u8>, Hata> {
    let mut cikti = Vec::new();
    let yazici = PngYazici::yeni(&mut cikti, secenek.clone())?;
    let mut w = yazici;
    for y in 0..secenek.yukseklik {
        w.yaz(&satir_uretici(y))?;
    }
    let _ = w.bitir()?;
    Ok(cikti)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gorsel::{Raster, KANAL};
    use crate::meta::{MetinKaydi, MetinTuru};
    use crate::test_yardimci::ac;

    fn dolu_govde(g: &mut Raster, uret: impl Fn(u32, u32) -> [u8; KANAL]) {
        for y in 0..g.yukseklik {
            for x in 0..g.genislik {
                g.piksel_ata(x, y, uret(x, y));
            }
        }
    }

    /// RGBA8 satırından doğrudan ham satır üretir.
    fn rgba_satir(govde: &Raster, y: u32) -> Vec<u8> {
        (0..govde.genislik)
            .flat_map(|x| govde.piksel(x, y))
            .collect()
    }

    #[test]
    fn imza_ve_ihdr_dogru_yazilir() {
        let mut cikti = Vec::new();
        let mut w = ac(PngYazici::yeni(
            &mut cikti,
            ac(PngSecenek::yeni(2, 1, 8, 6)),
        ));
        ac(w.yaz(&[0; 8]));
        ac(w.bitir());
        if cikti[..8] != crate::png::blok::PNG_IMZASI {
            panic!("imza yanlis: {:?}", &cikti[..8]);
        }
        if cikti[12..16] != *b"IHDR" {
            panic!("ilk blok IHDR olmali");
        }
    }

    #[test]
    fn iend_dogru_yazilir() {
        let mut cikti = Vec::new();
        let mut w = ac(PngYazici::yeni(
            &mut cikti,
            ac(PngSecenek::yeni(1, 1, 8, 6)),
        ));
        ac(w.yaz(&[1, 2, 3, 4]));
        ac(w.bitir());
        let son = &cikti[cikti.len() - 12..];
        if &son[4..8] != b"IEND" {
            panic!("son blok IEND olmali: {son:?}");
        }
        let beklenen = crate::png::blok::crc32_tip_ile(b"IEND", &[]);
        let yazan = u32::from_be_bytes([son[8], son[9], son[10], son[11]]);
        if beklenen != yazan {
            panic!("IEND CRC yanlis: {yazan:#x} != {beklenen:#x}");
        }
    }

    #[test]
    fn eksik_satir_bitirmede_hata_dondurur() {
        let mut cikti = Vec::new();
        let mut w = ac(PngYazici::yeni(
            &mut cikti,
            ac(PngSecenek::yeni(2, 2, 8, 6)),
        ));
        ac(w.yaz(&[0; 8]));
        match w.bitir() {
            Err(Hata::PngBaslikBozuk { ayrinti }) => {
                if !ayrinti.contains("1 tanesi yazildi") {
                    panic!("ayrinti sayilari bildirmeli: {ayrinti}");
                }
            }
            diger => panic!("eksik satir hatasi bekleniyordu: {diger:?}"),
        }
    }

    #[test]
    fn fazla_satir_hata_dondurur() {
        let mut cikti = Vec::new();
        let mut w = ac(PngYazici::yeni(
            &mut cikti,
            ac(PngSecenek::yeni(1, 1, 8, 6)),
        ));
        ac(w.yaz(&[0; 4]));
        match w.yaz(&[0; 4]) {
            Err(Hata::PngBaslikBozuk { .. }) => {}
            diger => panic!("fazla satir reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn yanlis_satir_uzunlugu_hata_dondurur() {
        let mut cikti = Vec::new();
        let mut w = ac(PngYazici::yeni(
            &mut cikti,
            ac(PngSecenek::yeni(2, 1, 8, 6)),
        ));
        match w.yaz(&[0; 3]) {
            Err(Hata::PngSatirKisa { beklenen, bulunan }) => {
                if beklenen != 8 || bulunan != 3 {
                    panic!("uzunluk bilgisi yanlis: {beklenen}/{bulunan}");
                }
            }
            diger => panic!("kisa satir reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn paletsiz_secenek_palet_tipi_reddeder() {
        match PngSecenek::yeni(2, 2, 8, 3) {
            Err(Hata::PngBaslikBozuk { ayrinti }) => {
                if !ayrinti.contains("palet zorunludur") {
                    panic!("ayrinti palet zorunlulugunu belirtmeli: {ayrinti}");
                }
            }
            diger => panic!("paletsiz renk tipi 3 reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn paletli_secenek_derinligi_paletten_turetir() {
        for (adet, beklenen) in [(2usize, 1u8), (4, 2), (16, 4), (200, 8)] {
            let palet = vec![[0u8, 0, 0]; adet];
            let s = ac(PngSecenek::paletli(2, 2, palet));
            if s.bit_derinligi != beklenen {
                panic!(
                    "{adet} palet girisi -> {beklenen} bit bekleniyordu, {} geldi",
                    s.bit_derinligi
                );
            }
            if s.renk_tipi != 3 {
                panic!("renk tipi 3 olmali");
            }
        }
    }

    #[test]
    fn paletli_secenek_giris_sinirlarini_kontrol_eder() {
        if PngSecenek::paletli(2, 2, Vec::new()).is_ok() {
            panic!("bos palet reddedilmeli");
        }
        if PngSecenek::paletli(2, 2, vec![[0u8, 0, 0]; 257]).is_ok() {
            panic!("257 girisli palet reddedilmeli");
        }
    }

    #[test]
    fn metin_blogu_yazilir() {
        let mut secenek = ac(PngSecenek::yeni(1, 1, 8, 6));
        secenek.metin.push(MetinKaydi {
            anahtar: "Title".to_string(),
            deger: "Deniz".to_string(),
            tur: MetinTuru::TExt,
        });
        let mut cikti = Vec::new();
        let mut w = ac(PngYazici::yeni(&mut cikti, secenek));
        ac(w.yaz(&[0; 4]));
        ac(w.bitir());
        if !cikti.windows(4).any(|p| p == b"tEXt" || p == b"Title") {
            panic!("tEXt blogu yazilmadi");
        }
    }

    #[test]
    fn gecersiz_metin_anahtari_hata_dondurur() {
        let mut secenek = ac(PngSecenek::yeni(1, 1, 8, 6));
        secenek.metin.push(MetinKaydi {
            anahtar: " Bos".to_string(),
            deger: "x".to_string(),
            tur: MetinTuru::TExt,
        });
        let mut cikti = Vec::new();
        let sonuc = PngYazici::yeni(&mut cikti, secenek).err();
        match sonuc {
            Some(Hata::MetaVeriSinir { .. }) => {}
            diger => panic!("gecersiz anahtar reddedilmeli, alinan: {diger:?}"),
        }
    }

    #[test]
    fn trns_blogu_yazilir() {
        let mut secenek = ac(PngSecenek::yeni(1, 1, 8, 0));
        secenek.trns = Some(vec![0u8, 0]);
        let mut cikti = Vec::new();
        let mut w = ac(PngYazici::yeni(&mut cikti, secenek));
        ac(w.yaz(&[0]));
        ac(w.bitir());
        if !cikti.windows(4).any(|p| p == b"tRNS") {
            panic!("tRNS blogu yazilmadi");
        }
    }

    #[test]
    fn png_kodla_gidis_donus_basar() {
        let mut g = ac(Raster::yeni_sifir(3, 2));
        dolu_govde(&mut g, |x, y| [(x * 80) as u8, (y * 120) as u8, 33, 255]);
        let secenek = ac(PngSecenek::yeni(3, 2, 8, 6));
        let baytlar = ac(png_kodla(&secenek, &mut |y| rgba_satir(&g, y)));
        let okunan = ac(crate::png::okuma::png_coz(&baytlar[..]));
        if okunan.govde != g {
            panic!("png_kodla gidis-donus farkli");
        }
    }

    #[test]
    fn cok_satirli_govde_idat_parcalarina_bolunur() {
        // 300x300 RGB ve sikistirilamaz (LCG) desen -> IDAT_PARCA (64 KiB)
        // birden cok kez asilir ve veri birden cok IDAT bloguna bolunur.
        let mut g = ac(Raster::yeni_sifir(300, 300));
        let mut tohum: u32 = 0x1234_5678;
        for y in 0..300u32 {
            for x in 0..300u32 {
                let sonraki = |t: &mut u32| {
                    *t = t.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (*t >> 16) as u8
                };
                let r = sonraki(&mut tohum);
                let gv = sonraki(&mut tohum);
                let b = sonraki(&mut tohum);
                g.piksel_ata(x, y, [r, gv, b, 255]);
            }
        }
        let secenek = ac(PngSecenek::yeni(300, 300, 8, 2));
        let baytlar = ac(png_kodla(&secenek, &mut |y| {
            (0..300)
                .flat_map(|x| {
                    let p = g.piksel(x, y);
                    [p[0], p[1], p[2]]
                })
                .collect()
        }));
        let idat_sayisi = baytlar.windows(4).filter(|p| *p == b"IDAT").count();
        if idat_sayisi < 2 {
            panic!("cok parcali IDAT bekleniyordu, {idat_sayisi} adet bulundu");
        }
        let okunan = ac(crate::png::okuma::png_coz(&baytlar[..]));
        if okunan.govde.piksel(299, 299) != g.piksel(299, 299) {
            panic!("cok parcali IDAT gidis-donus hatali");
        }
    }

    #[test]
    fn idat_parca_degeri_dokumantasyonla_uyumlu() {
        if IDAT_PARCA != 65_536 {
            panic!("IDAT parca degeri 64 KiB olmali");
        }
    }
}
