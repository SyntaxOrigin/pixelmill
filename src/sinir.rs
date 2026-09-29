//! Girdi boyutu ve blok boyutu icin guvenlik ust sinirlari.
//!
//! Bu degerler "bellek bombasi" korumasidir: bozuk veya kotu niyetli bir dosya
//! islemcinin bellegini tuketerek coktugunu onler. Ust sinir asildiginda islem
//! **sessizce kirpilmez**, `Hata` ile durdurulur ve nedeni rapora yazilir.
//!
//! Degerler README'nin "Yapilandirma" bolumunde belgelenmistir ve CLI uzerinden
//! degistirilemez; degistirilebilir bir ust sinir icin `--azami-piksel` bayragi
//! ileride eklenebilir.

/// Bir kenarin piksel cinsinden izin verilen en buyuk degeri.
pub const EN_FAZLA_KENAR: u32 = 100_000;

/// Toplam piksel sayisinin izin verilen en buyuk degeri (bellek bombasi korumasi).
pub const EN_FAZLA_PIKSEL: u64 = 200_000_000;

/// Tek bir PNG blogunun izin verilen en buyuk uzunlugu (bayt).
///
/// PNG spec bir blok icin `u32` uzunluga izin verir; bu deger pratikte her
/// gercekci IDAT'in cok ustundedir ve kotu niyetli `length` alanina karsi korur.
pub const EN_FAZLA_BLOK: u64 = 32 * 1024 * 1024;

/// Okunan tek bir meta veri segmentinin izin verilen en buyuk uzunlugu (bayt).
pub const EN_FAZLA_META_SEGMENT: u64 = 1024 * 1024;

/// Toplu gezintide izlenecek en buyuk dizin derinligi.
///
/// Sonsuz dongu veya simge baglanti selalerine karsi yuzey siniri.
pub const EN_FAZLA_DERINLIK: usize = 32;

/// Yeniden boyutlandirmada izin verilen en buyuk bicubic yaricap (piksel).
///
/// Bicubic icin 2 yeterlidir; deger sabit tutulur ki halka tamponu sabit kalsin.
pub const BICUBIC_YARICAP: i64 = 2;

/// Bicubic (Keys, `a = -0.5`) cekirdeginin bir noktadaki dort agirligini dondurur.
///
/// `t` `0.0..=1.0` arasi normalize konumdur. Dönen ağırlıklar sırasıyla kaynak
/// indislerine **-1, 0, +1, +2** karşılık gelir ve toplamları tam 1'dir.
///
/// Çekirdek (`W(x)` Keys `a = -0.5`):
///
/// ```text
/// W(x) = 1.5|x|³ - 2.5|x|² + 1              (0 < |x| < 1)
///      = -0.5|x|³ + 2.5|x|² - 4|x| + 2     (1 <= |x| < 2)
///      = 0                                (|x| >= 2)
/// ```
///
/// Negatif ağırlıklar (ringing) kasıtlıdır ve ağırlık toplamını korur.
pub(crate) fn bicubic_agirlik(t: f64) -> [f64; 4] {
    let d0 = 1.0 - t;
    let w0 = 1.5 * d0 * d0 * d0 - 2.5 * d0 * d0 + 1.0;
    let w1 = 1.5 * t * t * t - 2.5 * t * t + 1.0;
    let d2 = 1.0 + t;
    let w2 = -0.5 * d2 * d2 * d2 + 2.5 * d2 * d2 - 4.0 * d2 + 2.0;
    let d3 = 2.0 - t;
    let w3 = -0.5 * d3 * d3 * d3 + 2.5 * d3 * d3 - 4.0 * d3 + 2.0;
    [w0, w1, w2, w3]
}

/// Bicubic agirliklarinin toplami 1'e yakin mi diye dogrulayan yardimci (testler icin).
#[cfg(test)]
pub(crate) fn bicubic_agirlik_toplami(t: f64) -> f64 {
    bicubic_agirlik(t).iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bicubic_agirliklari_birim_toplamli() {
        for adim in 0..=10 {
            let t = f64::from(adim) / 10.0;
            let toplam = bicubic_agirlik_toplami(t);
            if (toplam - 1.0).abs() > 1e-9 {
                panic!("t={t} icin agirlik toplami 1 degil: {toplam}");
            }
        }
    }

    #[test]
    fn bicubic_tam_noktada_birim_dik() {
        // t = 0 iken yalnizca orta piksel (indeks 0) agir olmali.
        let agirlik = bicubic_agirlik(0.0);
        if (agirlik[1] - 1.0).abs() > 1e-12 {
            panic!("t=0 yanlis agirliklar: {agirlik:?}");
        }
        if agirlik[0].abs() > 1e-12 || agirlik[2].abs() > 1e-12 || agirlik[3].abs() > 1e-12 {
            panic!("t=0 yanlis agirliklar: {agirlik:?}");
        }
    }

    #[test]
    fn bicubic_bir_tam_kayma_noktasinda_birim_dik() {
        // t = 1 iken agirlik tam olarak bir kaynak piksel sola kayar: indis -1.
        let agirlik = bicubic_agirlik(1.0);
        if (agirlik[0] - 1.0).abs() > 1e-12 {
            panic!("t=1 yanlis agirliklar: {agirlik:?}");
        }
        if agirlik[1].abs() > 1e-12 || agirlik[2].abs() > 1e-12 || agirlik[3].abs() > 1e-12 {
            panic!("t=1 yanlis agirliklar: {agirlik:?}");
        }
    }

    #[test]
    fn bicubic_yarim_noktada_simetrik() {
        // t = 0.5 icin iki ic agirlik esit ve iki dis agirlik esit olmali.
        let agirlik = bicubic_agirlik(0.5);
        if (agirlik[0] - 0.5625).abs() > 1e-12 || (agirlik[1] - 0.5625).abs() > 1e-12 {
            panic!("t=0.5 ic agirliklar 0.5625 olmali: {agirlik:?}");
        }
        if (agirlik[2] + 0.0625).abs() > 1e-12 || (agirlik[3] + 0.0625).abs() > 1e-12 {
            panic!("t=0.5 dis agirliklar -0.0625 olmali (ringing): {agirlik:?}");
        }
    }

    #[test]
    fn sinirlar_birer_kiyim_deger() {
        if EN_FAZLA_KENAR != 100_000 || EN_FAZLA_PIKSEL != 200_000_000 {
            panic!("sinirlar README ile eslesmeli");
        }
        if EN_FAZLA_BLOK != 33_554_432 {
            panic!("blok siniri 32 MiB olmali");
        }
        if EN_FAZLA_META_SEGMENT != 1_048_576 {
            panic!("meta veri siniri 1 MiB olmali");
        }
        if BICUBIC_YARICAP != 2 {
            panic!("bicubic yaricapi 2 olmali");
        }
        if EN_FAZLA_DERINLIK != 32 {
            panic!("derinlik siniri 32 olmali");
        }
    }
}
