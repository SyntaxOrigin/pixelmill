//! PNG blok (chunk) yapisi ve CRC-32 hesabi.
//!
//! Bu modul PNG'nin en temel birimini ele alir: `uzunluk (4, big-endian)` +
//! `tip (4 bayt)` + `veri` + `CRC-32 (4, big-endian)`. CRC, **tip ve veri**
//! baytlarinin uzerinden hesaplanir (PNG spec, "Error detection").
//!
//! CRC-32 dongusu PNG spec'inin belirttigi refleks polinom
//! `0xEDB88320` ile burada **kendi tablomuzla** hesaplanir; harici bir
//! bagimlilik kullanilmaz.

use crate::hata::Hata;
use crate::sinir::EN_FAZLA_BLOK;

/// PNG dosya imzasi (8 bayt). Her PNG dosyasi bu diziyle baslamak zorundadir.
pub const PNG_IMZASI: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// CRC-32 tablosunu bir kez uretir.
///
/// Kaba kuvvet uretim: her bayt icin 8 kez refleks kaydirma.
fn crc_tablosu() -> [u32; 256] {
    let mut tablo = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            bit += 1;
        }
        tablo[i] = c;
        i += 1;
    }
    tablo
}

/// Byte tabloda CRC-32 hesaplar (PNG / zlib / gzip'in kullandigi refleks polinom).
///
/// `onceki` deger `0` olarak baslatilir ve her cagri bir onceki cagrinin sonucu
/// olarak verilebilir; boylece akis halinde hesaplanabilir.
#[must_use]
pub fn crc32(veri: &[u8], onceki: u32) -> u32 {
    let tablo = crc_tablosu();
    let mut crc = onceki ^ 0xFFFF_FFFF;
    for &b in veri {
        crc = tablo[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Iki blok tipini birlikte CRC'leyen kisa yol (`tip` + `veri`).
#[must_use]
pub fn crc32_tip_ile(tip: &[u8; 4], veri: &[u8]) -> u32 {
    let mut crc = crc32(tip, 0);
    crc = crc32(veri, crc);
    crc
}

/// Bir blogun bayt cinsinden toplam uzunlugu (uzunluk alani dahil degil).
#[must_use]
pub fn blok_uzunlugu(veri_uzunlugu: u32) -> u64 {
    12 + u64::from(veri_uzunlugu)
}

/// Blok tipini dort bayttan okunabilir metne cevirir (gecersiz baytlar `?`).
#[must_use]
pub fn tip_metin(tip: &[u8; 4]) -> String {
    tip.iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect()
}

/// Blok uzunlugunun guvenlik sinirini denetler ve `u32`'e cevirir.
///
/// # Hatalar
///
/// Uzunluk `EN_FAZLA_BLOK` sinirini asarsa `Hata::PngBlokCokBuyuk` doner.
pub fn uzunlugu_dogrula(tip: &[u8; 4], ham: u32) -> Result<usize, Hata> {
    let bayt = u64::from(ham);
    if bayt > EN_FAZLA_BLOK {
        return Err(Hata::PngBlokCokBuyuk {
            blok_tipi: tip_metin(tip),
            bayt,
            sinir: EN_FAZLA_BLOK,
        });
    }
    Ok(ham as usize)
}

/// Bir blok basligini (uzunluk + tip) ayristirir.
///
/// # Hatalar
///
/// Uzunluk sinirini asarsa hata doner.
pub fn basligi_ayir(ham: &[u8], ofset: usize) -> Result<(u32, [u8; 4]), Hata> {
    if ham.len() < ofset + 8 {
        return Err(Hata::PngBlokBozuk {
            blok_tipi: "?".to_string(),
            ayrinti: format!("blok basligi kisa (ofset {ofset}, elde {} bayt)", ham.len()),
        });
    }
    let uzunluk = u32::from_be_bytes([ham[ofset], ham[ofset + 1], ham[ofset + 2], ham[ofset + 3]]);
    let tip = [
        ham[ofset + 4],
        ham[ofset + 5],
        ham[ofset + 6],
        ham[ofset + 7],
    ];
    uzunlugu_dogrula(&tip, uzunluk)?;
    Ok((uzunluk, tip))
}

/// Tek bir blogu bayt dizisinden okur: baslik + veri + CRC, CRC dogrulanir.
///
/// # Hatalar
///
/// Blok kisa, CRC bozuk veya uzunluk sinirini asiyorsa hata doner.
pub fn blogu_oku(ham: &[u8], ofset: usize) -> Result<([u8; 4], &[u8], usize), Hata> {
    let (uzunluk, tip) = basligi_ayir(ham, ofset)?;
    let veri_uzunlugu = uzunlugu_dogrula(&tip, uzunluk)?;
    let toplam = blok_uzunlugu(uzunluk) as usize;
    if ham.len() < ofset + toplam {
        return Err(Hata::PngBlokBozuk {
            blok_tipi: tip_metin(&tip),
            ayrinti: format!(
                "blogun {} bayti elde, gereken {} (ofset {ofset})",
                ham.len().saturating_sub(ofset),
                toplam
            ),
        });
    }
    let veri = &ham[ofset + 8..ofset + 8 + veri_uzunlugu];
    let hesaplanan = crc32_tip_ile(&tip, veri);
    let dosyadan = u32::from_be_bytes([
        ham[ofset + 8 + veri_uzunlugu],
        ham[ofset + 9 + veri_uzunlugu],
        ham[ofset + 10 + veri_uzunlugu],
        ham[ofset + 11 + veri_uzunlugu],
    ]);
    if dosyadan != hesaplanan {
        return Err(Hata::PngCrcBozuk {
            blok_tipi: tip_metin(&tip),
            dosyadan,
            hesaplanan,
        });
    }
    Ok((tip, veri, ofset + toplam))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_yardimci::ac;

    /// PNG spec'inin referans CRC-32 degeri: "IEND" blogu 0xAE426082 olmali.
    /// Kaynak: PNG 1.2 spec, "4.1.3 Error detection" ve W3C test dosyalari.
    #[test]
    fn png_iend_blogunun_crcsi_referans_degerdedir() {
        let crc = crc32_tip_ile(b"IEND", &[]);
        if crc != 0xAE42_6082 {
            panic!("IEND CRC beklenen 0xAE426082, alinan {crc:#010x}");
        }
    }

    /// "IHDR" + sifir veri icin hesaplanan deger deterministik olmali.
    #[test]
    fn crc_akisi_parcali_hesapla_dagilabilir() {
        let veri: Vec<u8> = (0u8..=255).collect();
        let tek = crc32(&veri, 0);
        let parca1 = crc32(&veri[..100], 0);
        let parca2 = crc32(&veri[100..], parca1);
        if tek != parca2 {
            panic!("parcali CRC tek gecisli CRC ile ayni olmali: {tek} != {parca2}");
        }
    }

    #[test]
    fn crc_bos_girdi_icin_sifirdir() {
        if crc32(&[], 0) != 0 {
            panic!("bos girdi icin CRC 0 olmali");
        }
    }

    #[test]
    fn crc_bir_bit_degisiminde_degisir() {
        let a = crc32(b"hello world", 0);
        let b = crc32(b"hello worle", 0);
        if a == b {
            panic!("tek bayt farki CRC'yi degistirmeli");
        }
    }

    #[test]
    fn tip_metin_kontrol_karakterlerini_degistirir() {
        if tip_metin(b"IEND") != "IEND" {
            panic!("ASCII tip aynen korunmali");
        }
        if tip_metin(&[0x01, 0x02, 0x03, 0x04]) != "????" {
            panic!("kontrol baytleri '?' olmali");
        }
    }

    #[test]
    fn blok_uzunlugu_on_dort_bayt_ekler() {
        if blok_uzunlugu(0) != 12 {
            panic!("bos blok 12 bayt olmali");
        }
        if blok_uzunlugu(13) != 25 {
            panic!("13 baytlik blok 25 bayt olmali");
        }
    }

    #[test]
    fn imza_dogru_tanimlanmis() {
        if PNG_IMZASI[0] != 0x89 || &PNG_IMZASI[1..4] != b"PNG" {
            panic!("PNG imzasi tanimi bozuk");
        }
        if PNG_IMZASI[4..] != [0x0D, 0x0A, 0x1A, 0x0A] {
            panic!("PNG imzasi sonrasi baytlar yanlis");
        }
    }

    #[test]
    fn basligi_ayir_uzunluk_ve_tipi_ayiklar() {
        let mut ham = Vec::new();
        ham.extend_from_slice(&7u32.to_be_bytes());
        ham.extend_from_slice(b"tEXt");
        ham.extend_from_slice(&[0u8; 7]);
        let (uzunluk, tip) = ac(basligi_ayir(&ham, 0));
        if uzunluk != 7 || &tip != b"tEXt" {
            panic!("baslik ayristirilamadi: {uzunluk} / {tip:?}");
        }
    }

    #[test]
    fn kisa_baslik_hata_dondurur() {
        let ham = [0u8; 4];
        match basligi_ayir(&ham, 0) {
            Err(Hata::PngBlokBozuk { .. }) => {}
            diger => panic!("kisa baslik hata donmeli: {diger:?}"),
        }
    }

    #[test]
    fn asiri_uzunluk_sinir_reddi_verir() {
        let tip = *b"IDAT";
        match uzunlugu_dogrula(&tip, u32::MAX) {
            Err(Hata::PngBlokCokBuyuk { sinir, .. }) => {
                if sinir != EN_FAZLA_BLOK {
                    panic!("sinir bildirilmeli: {sinir}");
                }
            }
            diger => panic!("asiri uzunluk reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn blok_okuma_crc_dogrular() {
        let veri = b"abc";
        let crc = crc32_tip_ile(b"tEXt", veri);
        let mut ham = Vec::new();
        ham.extend_from_slice(&(veri.len() as u32).to_be_bytes());
        ham.extend_from_slice(b"tEXt");
        ham.extend_from_slice(veri);
        ham.extend_from_slice(&crc.to_be_bytes());
        let (tip, okunan, sonraki) = ac(blogu_oku(&ham, 0));
        if &tip != b"tEXt" || okunan != veri || sonraki != ham.len() {
            panic!("blok okunamadi: {tip:?} / {okunan:?} / {sonraki}");
        }
    }

    #[test]
    fn bozuk_crc_hata_dondurur() {
        let veri = b"abc";
        let mut ham = Vec::new();
        ham.extend_from_slice(&(veri.len() as u32).to_be_bytes());
        ham.extend_from_slice(b"tEXt");
        ham.extend_from_slice(veri);
        ham.extend_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
        match blogu_oku(&ham, 0) {
            Err(Hata::PngCrcBozuk {
                blok_tipi,
                dosyadan,
                ..
            }) => {
                if blok_tipi != "tEXt" || dosyadan != 0xDEAD_BEEF {
                    panic!("CRC hatasi beklenen degerlerle donmeli");
                }
            }
            diger => panic!("bozuk CRC hata donmeli: {diger:?}"),
        }
    }

    #[test]
    fn kisa_veri_hata_dondurur() {
        let ham = [0u8, 0, 0, 5, b't', b'E', b'X', b't', 1, 2];
        match blogu_oku(&ham, 0) {
            Err(Hata::PngBlokBozuk { blok_tipi, .. }) if blok_tipi == "tEXt" => {}
            diger => panic!("kisa veri hata donmeli: {diger:?}"),
        }
    }
}
