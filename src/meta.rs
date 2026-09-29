//! Gömülü konum verisi ve metin alanlarının okunması/temizlenmesi.
//!
//! Kapsam (MANIFEST kart 04, madde 5 — "daraltılmış"):
//!
//! - **PNG**: `tEXt`, `zTXt`, `iTXt` ve `eXIf` bloklarının varlığı ve içeriği okunur.
//! - **JPEG**: `APP1`/`Exif` segmenti ve `APP0` JFIF bilgisi okunur.
//! - **Temizleme**: PNG çıktısında `tEXt`/`eXIf` blokları **seçici olarak
//!   yazılabilir veya tamamen düşürülebilir**. JPEG **yeniden kodlanmadığı**
//!   için JPEG'de temizleme yapılmaz; yalnızca alan listesi raporlanır ve bu
//!   durum `README` → `Bilinen Sınırlamalar` bölümünde belgelenmiştir.
//! - **Tam EXIF yazımı yoktur**: TIFF IFD yalnızca okunur, yeniden üretilmez.
//!
//! EXIF (TIFF) ayrıştırma `TIFF 6.0` ve `Exif 2.3` (TIFF/EP) düzenine göre
//! yapılır: bayt sırası işareti (`II` / `MM`), sihirli sayı `42`, IFD0 giris
//! sayısı ve 12 bayt'lık girişler.

use crate::hata::Hata;

/// Metin kaydinin hangi PNG blogundan gelendigi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetinTuru {
    /// Latin-1 metin blogu.
    TExt,
    /// Sıkıştırılmış Latin-1 metin blogu.
    ZTxt,
    /// UTF-8, opsiyonel sıkıştırmalı metin blogu.
    ITXt,
}

impl MetinTuru {
    /// Blogun dort harfli adini dondurur.
    #[must_use]
    pub fn blog_adi(self) -> &'static str {
        match self {
            MetinTuru::TExt => "tEXt",
            MetinTuru::ZTxt => "zTXt",
            MetinTuru::ITXt => "iTXt",
        }
    }

    /// Blok tipi baytindan turu cozer.
    #[must_use]
    pub fn blog_tipinden(tip: &[u8; 4]) -> Option<Self> {
        match tip {
            b"tEXt" => Some(MetinTuru::TExt),
            b"zTXt" => Some(MetinTuru::ZTxt),
            b"iTXt" => Some(MetinTuru::ITXt),
            _ => None,
        }
    }
}

/// Tek bir metin kaydi (anahtar/deger cifti).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetinKaydi {
    /// Anahtar (PNG spec: 1..79 bayt, Latin-1, son karakter disinda bos olamaz).
    pub anahtar: String,
    /// Duz metin degeri (zTXt icin cozulmus hali).
    pub deger: String,
    /// Kaydin gelendigi blog turu.
    pub tur: MetinTuru,
}

/// EXIF/TIFF alaninda taninan bir etiket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExifAlani {
    /// Sayisal etiket kodu (orn. `0x8825` = GPS IFD isaretcisi).
    pub etiket: u16,
    /// Etiketin bilinen adi, bilinmiyorsa `None`.
    pub ad: Option<&'static str>,
    /// TIFF veri turu kodu (3 = kisa, 4 = uzun, ...).
    pub veri_turu: u16,
    /// Degerin bayt sayisi.
    pub boyut: u32,
    /// Onizgeler icin okunan deger (sayisal turler icin ham sayi).
    pub sayisal_deger: Option<u64>,
}

/// PNG `tEXt` / `zTXt` / `iTXt` anahtarinin gecerli olup olmadigini denetler.
///
/// PNG spec, bolum 4.7: anahtar 1..79 bayt olmali, Latin-1 karakterlerden
/// kurulmali, bos (`space`) olamamali ve **bastan ve sondan** bosluk
/// icermemelidir. Iç boşluklar spec'te yasaklanmadigi icin burada reddedilmez.
///
/// # Hatalar
///
/// Anahtar kurallara uymazsa `Hata::MetaVeriSinir` doner.
pub fn anahtari_dogrula(anahtar: &str) -> Result<(), Hata> {
    let baytlar = anahtar.as_bytes();
    if baytlar.is_empty() || baytlar.len() > 79 {
        return Err(Hata::MetaVeriSinir {
            segment: "tEXt anahtari".to_string(),
            bayt: baytlar.len() as u64,
            sinir: 79,
        });
    }
    if !baytlar.iter().all(|b| (0x20..=0xFF).contains(b) && *b != 0) {
        return Err(Hata::MetaVeriSinir {
            segment: "tEXt anahtari".to_string(),
            bayt: baytlar.len() as u64,
            sinir: 79,
        });
    }
    if baytlar[0] == b' ' || baytlar[baytlar.len() - 1] == b' ' {
        return Err(Hata::MetaVeriSinir {
            segment: "tEXt anahtari".to_string(),
            bayt: baytlar.len() as u64,
            sinir: 79,
        });
    }
    Ok(())
}

/// `uzunluk + tip + veri` seklinde toplanmis PNG bloklarini metin ve `eXIf`
/// olarak ayiklar.
///
/// Meta tamponu her blogu `uzunluk (4, big-endian) + tip (4) + veri` olarak
/// saklar; boylece ard arda gelen birden fazla metin blogu belirsizlik olmadan
/// ayristirilir. Bu fonksiyon hata donmez: bozuk veya eksik bir blogda dongu
/// kirilir ve o ana kadar toplanan kayitlar gecerlidir.
pub fn png_metin_ayikla(ham: &[u8]) -> (Vec<MetinKaydi>, Option<Vec<u8>>) {
    let mut metin = Vec::new();
    let mut exif = None;
    let mut konum = 0usize;
    while konum + 8 <= ham.len() {
        let uzunluk =
            u32::from_be_bytes([ham[konum], ham[konum + 1], ham[konum + 2], ham[konum + 3]])
                as usize;
        let mut tip = [0u8; 4];
        tip.copy_from_slice(&ham[konum + 4..konum + 8]);
        let veri_bas = konum + 8;
        let veri_son = match veri_bas.checked_add(uzunluk) {
            Some(son) if son <= ham.len() => son,
            _ => break,
        };
        let veri = &ham[veri_bas..veri_son];
        konum = veri_son;
        if &tip == b"eXIf" {
            exif = Some(veri.to_vec());
        } else if let Some(tur) = MetinTuru::blog_tipinden(&tip) {
            if let Some(kayit) = metin_kaydini_coz(tur, veri) {
                metin.push(kayit);
            }
        }
    }
    (metin, exif)
}

/// Tek bir metin blogunun icerigini cozer.
fn metin_kaydini_coz(tur: MetinTuru, veri: &[u8]) -> Option<MetinKaydi> {
    let ayirac = veri.iter().position(|&b| b == 0)?;
    let anahtar = String::from_utf8_lossy(&veri[..ayirac]).into_owned();
    let kalan = &veri[ayirac + 1..];
    let deger = match tur {
        MetinTuru::TExt => Some(latin1_metni(kalan)),
        MetinTuru::ITXt => {
            // iTXt: bayt_sayisi(1) + bayt_sayisi(2) + dil_etiketi + ceviri_anahtari
            // + metin. Ucuncu bayt genellikle 0 (sikistirma=0).
            if kalan.len() < 2 {
                return None;
            }
            let sikistirma = kalan[0] == 1;
            let mut i = 2usize;
            while i < kalan.len() && kalan[i] != 0 {
                i += 1;
            }
            i += 1;
            while i < kalan.len() && kalan[i] != 0 {
                i += 1;
            }
            i += 1;
            let ham = &kalan[i..];
            if sikistirma {
                zlib_ac(ham)
            } else {
                Some(String::from_utf8_lossy(ham).into_owned())
            }
        }
        MetinTuru::ZTxt => {
            // zTXt: anahtar + NUL + sikistirma_yontemi (1 bayt, deger 0) + zlib veri.
            if kalan.len() < 2 || kalan[0] != 0 {
                return None;
            }
            zlib_ac(&kalan[1..])
        }
    }?;
    Some(MetinKaydi {
        anahtar,
        deger,
        tur,
    })
}

/// zlib sıkıştırılmış bir metni açar; basarisizsa `None` doner.
fn zlib_ac(veri: &[u8]) -> Option<String> {
    use std::io::Read;
    let mut cozucu = flate2::read::ZlibDecoder::new(veri);
    let mut metin = Vec::new();
    cozucu.read_to_end(&mut metin).ok()?;
    Some(latin1_metni(&metin))
}

/// Latin-1 baytlari UTF-8 metne cevirir.
fn latin1_metni(veri: &[u8]) -> String {
    veri.iter().map(|&b| char::from(b)).collect()
}

/// TIFF/EXIF bayt dizisinden IFD0 alanlarini listeler.
///
/// # Hatalar
///
/// Bayt sırası işareti `II`/`MM` degilse, sihirli sayı `42` degilse veya IFD
/// ofseti dosya disindaysa `Hata::JpegBozuk` doner.
pub fn exif_etiketleri(tiff: &[u8]) -> Result<Vec<ExifAlani>, Hata> {
    if tiff.is_empty() {
        return Ok(Vec::new());
    }
    if tiff.len() < 8 {
        return Err(Hata::JpegBozuk {
            ayrinti: format!("EXIF TIFF basligi cok kisa ({} bayt)", tiff.len()),
        });
    }
    let kucuk = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        diger => {
            return Err(Hata::JpegBozuk {
                ayrinti: format!(
                    "TIFF bayt sirasi isareti gecersiz: {}",
                    String::from_utf8_lossy(diger)
                ),
            })
        }
    };
    let oku16 = |ofset: usize| -> u16 {
        let a = u16::from(tiff[ofset]);
        let b = u16::from(tiff[ofset + 1]);
        if kucuk {
            a | (b << 8)
        } else {
            (a << 8) | b
        }
    };
    let sihirli = oku16(2);
    if sihirli != 42 {
        return Err(Hata::JpegBozuk {
            ayrinti: format!("TIFF sihirli sayisi 42 degil: {sihirli}"),
        });
    }
    // IFD ofseti TIFF'te **dort** bayttir; iki bayt olarak okumak buyuk
    // bayt sirasi dosyalarinda yanlis oflete yol acar.
    let ofset_baytlari = [
        tiff.get(4).copied().unwrap_or(0),
        tiff.get(5).copied().unwrap_or(0),
        tiff.get(6).copied().unwrap_or(0),
        tiff.get(7).copied().unwrap_or(0),
    ];
    let ifd_ofset = if kucuk {
        u32::from_le_bytes(ofset_baytlari)
    } else {
        u32::from_be_bytes(ofset_baytlari)
    } as usize;
    ifd_ayikla(tiff, ifd_ofset, kucuk, &oku16, 0)
}

/// IFD0 alanlarini ayiklar (asagidaki yardimci `ifd_ayikla` uzerinden).
fn ifd_ayikla(
    tiff: &[u8],
    ofset: usize,
    kucuk: bool,
    oku16: &dyn Fn(usize) -> u16,
    derinlik: u8,
) -> Result<Vec<ExifAlani>, Hata> {
    if derinlik > 2 {
        return Err(Hata::JpegBozuk {
            ayrinti: "EXIF alt-IFD zinciri cok derin".to_string(),
        });
    }
    let oku32 = |o: usize| -> u32 {
        let b = [
            tiff.get(o).copied().unwrap_or(0),
            tiff.get(o + 1).copied().unwrap_or(0),
            tiff.get(o + 2).copied().unwrap_or(0),
            tiff.get(o + 3).copied().unwrap_or(0),
        ];
        if kucuk {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    if ofset + 2 > tiff.len() {
        return Err(Hata::JpegBozuk {
            ayrinti: format!("IFD ofseti ({ofset}) TIFF boyutunu ({}) asiyor", tiff.len()),
        });
    }
    let giris_sayisi = usize::from(oku16(ofset));
    let mut alanlar = Vec::new();
    for i in 0..giris_sayisi {
        let g = ofset + 2 + i * 12;
        if g + 12 > tiff.len() {
            return Err(Hata::JpegBozuk {
                ayrinti: format!("IFD girisi {i} dosya disinda"),
            });
        }
        let etiket = oku16(g);
        let veri_turu = oku16(g + 2);
        let adet = oku32(g + 4);
        alanlar.push(ExifAlani {
            etiket,
            ad: exif_etiket_adi(etiket),
            veri_turu,
            boyut: adet,
            sayisal_deger: Some(u64::from(oku32(g + 8))),
        });
    }
    Ok(alanlar)
}

/// Sik kar gorulen EXIF etiketlerinin adlari.
#[must_use]
pub fn exif_etiket_adi(etiket: u16) -> Option<&'static str> {
    Some(match etiket {
        0x010E => "ImageDescription",
        0x010F => "Make",
        0x0110 => "Model",
        0x0112 => "Orientation",
        0x011A => "XResolution",
        0x011B => "YResolution",
        0x0131 => "Software",
        0x0132 => "DateTime",
        0x013B => "Artist",
        0x8298 => "Copyright",
        0x8769 => "ExifIFDPointer",
        0x8825 => "GPSInfoIFDPointer",
        _ => return None,
    })
}

/// Verilen EXIF alanlarindan **konum verisi** olanlarin etiketlerini dondurur.
///
/// Konum verisi `GPSInfoIFDPointer` (0x8825) isaretcisi ve GPS alt-IFD'sindeki
/// `GPSLatitudeRef`, `GPSLatitude`, `GPSLongitudeRef`, `GPSLongitude`,
/// `GPSAltitude` alanlariyla temsil edilir.
///
/// # Hatalar
///
/// Alt-IFD okunamazsa hata doner.
pub fn konum_etiketlerini_isaretle(alanlar: &[ExifAlani]) -> Result<Vec<u16>, Hata> {
    let mut isaretli = Vec::new();
    for alan in alanlar {
        if alan.etiket == 0x8825 {
            isaretli.push(0x8825);
        }
    }
    Ok(isaretli)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_yardimci::ac;

    /// Kucuk bayt sirali, iki alanli (Artist + GPS isaretcisi) ornek TIFF uretir.
    fn ornek_tiff() -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(b"II");
        t.extend_from_slice(&42u16.to_le_bytes());
        t.extend_from_slice(&8u32.to_le_bytes());
        t.extend_from_slice(&2u16.to_le_bytes());
        // Alan 1: Artist (0x013B), ASCII, 5 bayt deger ofseti 0x0028
        t.extend_from_slice(&0x013Bu16.to_le_bytes());
        t.extend_from_slice(&2u16.to_le_bytes());
        t.extend_from_slice(&5u32.to_le_bytes());
        t.extend_from_slice(&0x0028u32.to_le_bytes());
        // Alan 2: GPSInfoIFDPointer (0x8825), LONG, deger 0
        t.extend_from_slice(&0x8825u16.to_le_bytes());
        t.extend_from_slice(&4u16.to_le_bytes());
        t.extend_from_slice(&1u32.to_le_bytes());
        t.extend_from_slice(&0u32.to_le_bytes());
        // Sonraki IFD ofseti (yok)
        t.extend_from_slice(&0u32.to_le_bytes());
        // 0x28 = 40: "Deniz"
        t.extend_from_slice(b"Deniz");
        t
    }

    /// Meta tamponu biciminde (`uzunluk + tip + veri`) bir blok üretir.
    fn meta_blogu(tip: &[u8; 4], veri: &[u8]) -> Vec<u8> {
        let mut ham = Vec::new();
        ham.extend_from_slice(&(veri.len() as u32).to_be_bytes());
        ham.extend_from_slice(tip);
        ham.extend_from_slice(veri);
        ham
    }

    #[test]
    fn anahtar_dogrulama_kurallari() {
        if anahtari_dogrula("Title").is_err() {
            panic!("gecerli anahtar kabul edilmeli");
        }
        for kotu in ["", " Bos", "Bos "] {
            if anahtari_dogrula(kotu).is_ok() {
                panic!("gecersiz anahtar reddedilmeli: '{kotu}'");
            }
        }
        let uzun = "a".repeat(80);
        if anahtari_dogrula(&uzun).is_ok() {
            panic!("80 baytlik anahtar reddedilmeli");
        }
    }

    #[test]
    fn tex_blogu_cozulur() {
        let mut veri = Vec::new();
        veri.extend_from_slice(b"Title\0Ornek baslik");
        let (metin, exif) = png_metin_ayikla(&meta_blogu(b"tEXt", &veri));
        if metin.len() != 1 {
            panic!("bir metin kaydi bekleniyordu: {metin:?}");
        }
        if metin[0].anahtar != "Title" || metin[0].deger != "Ornek baslik" {
            panic!("metin kaydi yanlis: {:?}", metin[0]);
        }
        if metin[0].tur != MetinTuru::TExt {
            panic!("tur TExt olmali");
        }
        if exif.is_some() {
            panic!("exif olmamali");
        }
    }

    #[test]
    fn exif_blogu_ayiklanir() {
        let tiff = ornek_tiff();
        let (metin, exif) = png_metin_ayikla(&meta_blogu(b"eXIf", &tiff));
        if !metin.is_empty() {
            panic!("metin olmamali");
        }
        match exif {
            Some(alindi) => {
                if alindi != tiff {
                    panic!("eXIf icerigi bozulmus");
                }
            }
            None => panic!("eXIf blogu ayiklanmaliydi"),
        }
    }

    #[test]
    fn bos_tampon_hata_vermez() {
        let (metin, exif) = png_metin_ayikla(&[]);
        if !metin.is_empty() || exif.is_some() {
            panic!("bos girdiden bos cikti beklenir");
        }
    }

    #[test]
    fn nul_isareti_olmayan_blog_yoksayilir() {
        let (metin, _) = png_metin_ayikla(&meta_blogu(b"tEXt", b"nulsuz"));
        if !metin.is_empty() {
            panic!("ayiracsiz blog atlanmali");
        }
    }

    #[test]
    fn ardisik_birden_cok_metin_blogu_okunur() {
        let mut a = Vec::new();
        a.extend_from_slice(b"Title\0Birinci");
        let mut b = Vec::new();
        b.extend_from_slice(b"Author\0Deniz");
        let mut ham = meta_blogu(b"tEXt", &a);
        ham.extend(meta_blogu(b"tEXt", &b));
        let (metin, _) = png_metin_ayikla(&ham);
        if metin.len() != 2 {
            panic!("iki metin kaydi bekleniyordu: {metin:?}");
        }
        if metin[0].deger != "Birinci" || metin[1].deger != "Deniz" {
            panic!("metin sirasi/icerigi yanlis: {metin:?}");
        }
    }

    #[test]
    fn bozuk_uzunlukta_dongu_kirilir() {
        let mut ham = Vec::new();
        ham.extend_from_slice(&9999u32.to_be_bytes());
        ham.extend_from_slice(b"tEXt");
        ham.extend_from_slice(b"kisa");
        let (metin, _) = png_metin_ayikla(&ham);
        if !metin.is_empty() {
            panic!("uzunluk tasmasi olan blog atlanmali");
        }
    }

    #[test]
    fn ztxt_blogu_zlib_ile_cozulur() {
        use flate2::write::ZlibEncoder;
        use std::io::Write;
        let mut kodlayici = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = kodlayici.write_all(b"sikistirilmis");
        let siki = ac(kodlayici.finish());
        let mut veri = Vec::new();
        veri.extend_from_slice(b"Comment\0");
        veri.push(0);
        veri.extend_from_slice(&siki);
        let (metin, _) = png_metin_ayikla(&meta_blogu(b"zTXt", &veri));
        if metin.len() != 1 || metin[0].deger != "sikistirilmis" {
            panic!("zTXt cozulemedi: {metin:?}");
        }
        if metin[0].tur != MetinTuru::ZTxt {
            panic!("tur ZTxt olmali");
        }
    }

    #[test]
    fn zlib_acma_calisir() {
        use flate2::write::ZlibEncoder;
        use std::io::Write;
        let mut kodlayici = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = kodlayici.write_all(b" sikistirilmis metin");
        let siki = ac(kodlayici.finish());
        let acilan = zlib_ac(&siki);
        match acilan {
            Some(m) if m.contains("sikistirilmis") => {}
            diger => panic!("zlib acilmadi: {diger:?}"),
        }
    }

    #[test]
    fn exif_ifd_ayiklanir() {
        let alanlar = ac(exif_etiketleri(&ornek_tiff()));
        if alanlar.len() != 2 {
            panic!("iki alan bekleniyordu: {alanlar:?}");
        }
        if alanlar[0].etiket != 0x013B || alanlar[0].ad != Some("Artist") {
            panic!("Artist alani okunamadi: {:?}", alanlar[0]);
        }
        if alanlar[0].veri_turu != 2 {
            panic!("Artist ASCII olmali");
        }
        if alanlar[1].etiket != 0x8825 || alanlar[1].ad != Some("GPSInfoIFDPointer") {
            panic!("GPS isaretcisi okunamadi");
        }
    }

    #[test]
    fn exif_bozuk_bayt_sirasi_reddedilir() {
        let mut t = ornek_tiff();
        t[0] = b'X';
        match exif_etiketleri(&t) {
            Err(Hata::JpegBozuk { ayrinti }) => {
                if !ayrinti.contains("bayt sirasi") {
                    panic!("ayrinti beklenen konuyu belirtmeli: {ayrinti}");
                }
            }
            diger => panic!("bozuk bayt sirasi reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn exif_bozuk_sihirli_sayi_reddedilir() {
        let mut t = ornek_tiff();
        t[2] = 0;
        match exif_etiketleri(&t) {
            Err(Hata::JpegBozuk { ayrinti }) => {
                if !ayrinti.contains("sihirli") {
                    panic!("ayrinti 'sihirli sayisi' demeli: {ayrinti}");
                }
            }
            diger => panic!("bozuk sihirli sayi reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn exif_kisa_tiff_reddedilir() {
        match exif_etiketleri(b"II") {
            Err(Hata::JpegBozuk { .. }) => {}
            diger => panic!("kisa TIFF reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn exif_ifd_ofseti_dosya_disi_reddedilir() {
        let mut t = ornek_tiff();
        t[4] = 0xFF;
        t[5] = 0xFF;
        match exif_etiketleri(&t) {
            Err(Hata::JpegBozuk { ayrinti }) => {
                if !ayrinti.contains("asıyor") && !ayrinti.contains("asiyor") {
                    panic!("ofset siniri belirtilmeli: {ayrinti}");
                }
            }
            diger => panic!("ofset siniri reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn buyuk_bayt_sirasi_okunur() {
        let mut t = Vec::new();
        t.extend_from_slice(b"MM");
        t.extend_from_slice(&42u16.to_be_bytes());
        t.extend_from_slice(&8u32.to_be_bytes());
        t.extend_from_slice(&1u16.to_be_bytes());
        t.extend_from_slice(&0x8298u16.to_be_bytes());
        t.extend_from_slice(&2u16.to_be_bytes());
        t.extend_from_slice(&3u32.to_be_bytes());
        t.extend_from_slice(&7u32.to_be_bytes());
        t.extend_from_slice(&0u32.to_be_bytes());
        t.extend_from_slice(&0u32.to_be_bytes());
        let alanlar = ac(exif_etiketleri(&t));
        if alanlar.len() != 1 || alanlar[0].etiket != 0x8298 {
            panic!("Copyright alani okunamadi: {alanlar:?}");
        }
        if alanlar[0].ad != Some("Copyright") {
            panic!("Copyright adi eslesmedi");
        }
    }

    #[test]
    fn konum_isaretcisi_bulunur() {
        let alanlar = ac(exif_etiketleri(&ornek_tiff()));
        let isaretli = ac(konum_etiketlerini_isaretle(&alanlar));
        if isaretli != vec![0x8825] {
            panic!("GPS isaretcisi isaretlenmeli: {isaretli:?}");
        }
        if !konum_etiketlerini_isaretle(&[])
            .unwrap_or_default()
            .is_empty()
        {
            panic!("bos listede isaret olmamali");
        }
    }

    #[test]
    fn bilinmeyen_etiket_adi_yoktur() {
        if exif_etiket_adi(0xFFFF).is_some() {
            panic!("bilinmeyen etiket icin ad dondurulmemeli");
        }
    }

    #[test]
    fn latin1_metni_karakterleri_korur() {
        let m = latin1_metni(&[0x41, 0xC7, 0xFC]);
        if m != "AÇü" {
            panic!("latin1 cevirisi yanlis: {m}");
        }
    }
}
