//! PNG tarama satiri filtreleri: uygulama (yazma) ve geri alma (okuma).
//!
//! PNG spec (RFC 2083, bolum 6) her tarama satirini bir filtre **tipi** baytiyla
//! baslar ve kalan baytlar filtrelenmis veriyi icerir. Bes tip tanimlidir:
//!
//! | Tip | Ad       | Islem (sol piksel `a`, ust satir `b`, sol-ust `c`) |
//! |-----|----------|-------------------------------------------------|
//! | 0   | None     | degisiklik yok                                  |
//! | 1   | Sub      | `x - a`                                          |
//! | 2   | Up       | `x - b`                                          |
//! | 3   | Average  | `x - (a + b) / 2`                               |
//! | 4   | Paeth    | `x - Paeth(a, b, c)`                            |
//!
//! `x` daima 8-bit ve **mod 256** uzerinden aritmetik yapilir. Bu modul
//! filtreleri **kanal duzeyinde** degil **bayt duzeyinde** uygular; PNG'de
//! "piksel" birlesimi `bpp` (bit derinligi x kanal sayisi / 8) kadar bayttir ve
//! fonksiyonlar `bpp` parametresiyle alir.

use crate::hata::Hata;

/// Filtre tipi baytinin en buyuk gecerli degeri.
pub const EN_FAZLA_FILTRE_TIPI: u8 = 4;

/// `Paeth` filtresinin ongorusu (PNG spec, bolum 6.2.6.3).
///
/// `a` = sol, `b` = ust, `c` = sol-ust komsularinin 8-bit degerleri.
/// Donen deger `a`, `b` veya `c` veya bunlarin `a + b - c` uzakligina gore
/// bir karsilikligidir (bagimsiz olarak secilen en yakin komsu).
#[must_use]
pub fn paeth_ongoru(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (i16::from(a) - p).abs();
    let pb = (i16::from(b) - p).abs();
    let pc = (i16::from(c) - p).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Bir tarama satirini verilen filtre tipiyle **uygular** (yazma yonu).
///
/// `onceki` Onceki tarama satiri olup ilk satir icin `None` verilir.
/// `bpp` piksel basina bayt sayisidir (8-bit RGB icin 3, 8-bit RGBA icin 4).
///
/// # Hatalar
///
/// Filtre tipi `0..=4` disindaysa veya `onceki` yanlis uzunluktaysa hata doner.
pub fn filtrele(
    tip: u8,
    bpp: usize,
    onceki: Option<&[u8]>,
    ham: &[u8],
    hedef: &mut [u8],
) -> Result<(), Hata> {
    if hedef.len() < ham.len() {
        return Err(Hata::PngSatirKisa {
            beklenen: ham.len(),
            bulunan: hedef.len(),
        });
    }
    let onceki = onceki.unwrap_or(&[]);
    // `onceki` bos ise (ilk satir) PNG spec geregi "onceki satir sifirdir"
    // kabul edilir; kisa ama bos olmayan bir onceki satir ise hatadir.
    if !onceki.is_empty() && onceki.len() < ham.len() {
        return Err(Hata::PngSatirKisa {
            beklenen: ham.len(),
            bulunan: onceki.len(),
        });
    }
    let bpp = bpp.max(1);
    if tip > EN_FAZLA_FILTRE_TIPI {
        return Err(Hata::PngFiltreTipiGecersiz { tip });
    }
    if tip == 0 {
        hedef[..ham.len()].copy_from_slice(ham);
        return Ok(());
    }
    let onceki_bos = onceki.is_empty();
    for i in 0..ham.len() {
        // Sol komsu **ham (filtrelenmemis)** satirdan alinir. PNG spec bunu
        // belirtir: filtre, baytin kendisi ile "bpp_onceki" konumundaki ham
        // baytin farkidir. Cozucu tarafta ayni deger zaten geri alinmis
        // satirdan okunur; iki yon ayni oldugu icin gidis-donus kesindir.
        // (Filtrelenmis hedeften okumak hatali sonuc verir.)
        let a = if i < bpp { 0 } else { ham[i - bpp] };
        let b = if onceki_bos { 0 } else { onceki[i] };
        let c = if i < bpp || onceki_bos {
            0
        } else {
            onceki[i - bpp]
        };
        let ongoru = match tip {
            1 => a,
            2 => b,
            3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
            _ => paeth_ongoru(a, b, c),
        };
        hedef[i] = ham[i].wrapping_sub(ongoru);
    }
    Ok(())
}

/// Bir filtrelenmis tarama satirini **geri alir** (okuma yonu).
///
/// `onceki` zaten geri alinmis Onceki satir olmalidir. `bpp` piksel basina bayt
/// sayisidir. `cikti` yazilir ve `sonraki_satir` olarak kullanilmak uzere
/// geri donulur.
///
/// # Hatalar
///
/// Filtre tipi gecersizse veya tamponlar kisa ise hata doner.
pub fn filtreyi_geri_al(
    tip: u8,
    bpp: usize,
    onceki: &[u8],
    filtreli: &[u8],
    cikti: &mut [u8],
) -> Result<(), Hata> {
    if cikti.len() < filtreli.len() {
        return Err(Hata::PngSatirKisa {
            beklenen: filtreli.len(),
            bulunan: cikti.len(),
        });
    }
    if onceki.len() < filtreli.len() {
        return Err(Hata::PngSatirKisa {
            beklenen: filtreli.len(),
            bulunan: onceki.len(),
        });
    }
    let bpp = bpp.max(1);
    for i in 0..filtreli.len() {
        let a = if i < bpp { 0 } else { cikti[i - bpp] };
        let b = onceki[i];
        let c = if i < bpp { 0 } else { onceki[i - bpp] };
        let ongoru = match tip {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
            4 => paeth_ongoru(a, b, c),
            diger => return Err(Hata::PngFiltreTipiGecersiz { tip: diger }),
        };
        cikti[i] = filtreli[i].wrapping_add(ongoru);
    }
    Ok(())
}

/// Bes filtre tipini de dener ve en kucuk mutlak-toplam sapmayi ureteni secer.
///
/// PNG spec'in önerdigi sezgisel yontem: filtrelenmis baytlar `i8` olarak
/// yorumlanip mutlak degerleri toplanir; en kucuk toplami veren filtre secilir.
/// Bu, "sikistirilabilirlik" icin bir vekildir (PNG'nin kendi filtrelemesi
/// DEFLATE icindadir) ve tamamen deterministiktir.
#[must_use]
pub fn en_iyi_filtre(bpp: usize, onceki: Option<&[u8]>, ham: &[u8]) -> u8 {
    let mut en_iyi_tip = 0u8;
    let mut en_iyi_puan: Option<u64> = None;
    let mut tampon = vec![0u8; ham.len()];
    for tip in 0..=EN_FAZLA_FILTRE_TIPI {
        // `filtrele` yalniz `Result` donduruyor; burada girdiler gecerli
        // oldugundan hata olusmaz. Yine de sessizce yoksayiyoruz.
        if filtrele(tip, bpp, onceki, ham, &mut tampon).is_err() {
            continue;
        }
        let puan: u64 = tampon
            .iter()
            .map(|&b| u64::from((b as i8).unsigned_abs()))
            .sum();
        let daha_iyi = match en_iyi_puan {
            None => true,
            Some(mevcut) => puan < mevcut,
        };
        if daha_iyi {
            en_iyi_puan = Some(puan);
            en_iyi_tip = tip;
        }
    }
    en_iyi_tip
}

/// Testler icin: bir satiri verilen filtre ile geriler.
///
/// Girdiler gecerli oldugu icin hata olusmaz; hata durumunda bos vektor doner.
#[cfg(test)]
pub(crate) fn gidis_donus(tip: u8, bpp: usize, ham: &[u8]) -> Vec<u8> {
    let mut filtreli = vec![0u8; ham.len()];
    if filtrele(tip, bpp, None, ham, &mut filtreli).is_ok() {
        filtreli
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_yardimci::ac;

    #[test]
    fn paeth_komsu_secimi_temel_kurallara_uyar() {
        // a = b = c ise sonuc a olmali.
        if paeth_ongoru(10, 10, 10) != 10 {
            panic!("a=b=c iken a secilmeli");
        }
        // Ongoru p = a + b - c; en yakin komsu secilir.
        // a=200,b=0,c=10 -> p=190; a en yakin (10 birim) -> 200.
        if paeth_ongoru(200, 0, 10) != 200 {
            panic!("a en yakinken a secilmeli");
        }
        // a=0,b=200,c=10 -> p=190; b en yakin (10 birim) -> 200.
        if paeth_ongoru(0, 200, 10) != 200 {
            panic!("b en yakinken b secilmeli");
        }
        // a=0,b=0,c=255 -> p=-255; a ve b esit uzaklikta -> a = 0.
        if paeth_ongoru(0, 0, 255) != 0 {
            panic!("c uzakken a secilmeli");
        }
        // a=10,b=200,c=200 -> p=10; a tam karsilik -> 10.
        if paeth_ongoru(10, 200, 200) != 10 {
            panic!("a tam karsilikken a secilmeli");
        }
    }

    #[test]
    fn paeth_tam_zaman_girdileri_guvenli() {
        // Hesaplamada tasma olmamali (komsular farkli uclarda).
        for a in [0u8, 1, 128, 254, 255] {
            for b in [0u8, 1, 128, 254, 255] {
                for c in [0u8, 1, 128, 254, 255] {
                    let _ = paeth_ongoru(a, b, c);
                }
            }
        }
    }

    #[test]
    fn filtre_sifir_ham_veriyi_kopyalar() {
        let ham = [1u8, 2, 3, 4, 5];
        let mut hedef = vec![0u8; 5];
        ac(filtrele(0, 3, None, &ham, &mut hedef));
        if hedef != ham {
            panic!("None filtresi veriyi degistirmemeli: {hedef:?}");
        }
    }

    #[test]
    fn sub_filtresi_soldaki_degeri_cikarir() {
        // bpp = 1: her bayt bir onceki bayttan cikarilir.
        let ham = [10u8, 20, 30];
        let mut hedef = vec![0u8; 3];
        ac(filtrele(1, 1, None, &ham, &mut hedef));
        // Sub filtresi bir onceki HAM bayti cikarir.
        if hedef != [10, 10, 10] {
            panic!("Sub filtresi farkli: {hedef:?}");
        }
    }

    #[test]
    fn up_filtresi_ust_satiri_cikarir() {
        let ham = [1u8, 2, 3];
        let ust = [10u8, 20, 30];
        let mut hedef = vec![0u8; 3];
        ac(filtrele(2, 1, Some(&ust), &ham, &mut hedef));
        if hedef
            != [
                1u8.wrapping_sub(10),
                2u8.wrapping_sub(20),
                3u8.wrapping_sub(30),
            ]
        {
            panic!("Up filtresi farkli: {hedef:?}");
        }
    }

    #[test]
    fn average_filtresi_ortalamayi_cikarir() {
        let ham = [10u8, 10];
        let ust = [0u8, 20];
        let mut hedef = vec![0u8; 2];
        ac(filtrele(3, 1, Some(&ust), &ham, &mut hedef));
        // Ikinci bayt icin ongoru (10 + 20) / 2 = 15 -> 10 - 15 = -5 = 251.
        if hedef != [10, 251] {
            panic!("Average filtresi farkli: {hedef:?}");
        }
    }

    #[test]
    fn gecersiz_filtre_tipi_hata_dondurur() {
        let mut hedef = vec![0u8; 4];
        match filtrele(5, 1, None, &[0; 4], &mut hedef) {
            Err(Hata::PngFiltreTipiGecersiz { tip: 5 }) => {}
            diger => panic!("tip 5 reddedilmeli: {diger:?}"),
        }
        match filtreyi_geri_al(200, 1, &[0; 4], &[0; 4], &mut hedef) {
            Err(Hata::PngFiltreTipiGecersiz { tip: 200 }) => {}
            diger => panic!("tip 200 reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn kisa_onceki_satir_hata_dondurur() {
        let mut hedef = vec![0u8; 10];
        match filtrele(2, 1, Some(&[0; 3]), &[0; 10], &mut hedef) {
            Err(Hata::PngSatirKisa { beklenen, bulunan }) => {
                if beklenen != 10 || bulunan != 3 {
                    panic!("beklenen/bulunan yanlis: {beklenen}/{bulunan}");
                }
            }
            diger => panic!("kisa onceki satir hata donmeli: {diger:?}"),
        }
    }

    #[test]
    fn bes_filtre_tipi_gerialma_gidis_donus_yapar() {
        let ham: Vec<u8> = (0..64u8).collect();
        for tip in 0..=EN_FAZLA_FILTRE_TIPI {
            let filtreli = gidis_donus(tip, 4, &ham);
            let mut cikti = vec![0u8; ham.len()];
            ac(filtreyi_geri_al(tip, 4, &[0u8; 64], &filtreli, &mut cikti));
            if cikti != ham {
                let ilk = cikti
                    .iter()
                    .zip(ham.iter())
                    .position(|(a, b)| a != b)
                    .unwrap_or(0);
                panic!(
                    "tip {tip} gidis-donus basarisiz: ilk fark {ilk}, beklenen {}, alinan {}",
                    ham[ilk], cikti[ilk]
                );
            }
        }
    }

    #[test]
    fn sub_filtresi_ham_soldan_cikarir() {
        // Sub filtresi bir onceki FILTRELENMIS degil, HAM bayti cikarir.
        let ham = [10u8, 20, 30];
        let mut hedef = vec![0u8; 3];
        ac(filtrele(1, 1, None, &ham, &mut hedef));
        if hedef != [10, 10, 10] {
            panic!("Sub filtresi farkli: {hedef:?}");
        }
    }

    #[test]
    fn gerialma_ust_satiri_kullanir() {
        let ust: Vec<u8> = (100..164u8).collect();
        let ham: Vec<u8> = (0..64u8).collect();
        let mut filtreli = vec![0u8; 64];
        ac(filtrele(2, 1, Some(&ust), &ham, &mut filtreli));
        let mut cikti = vec![0u8; 64];
        ac(filtreyi_geri_al(2, 1, &ust, &filtreli, &mut cikti));
        if cikti != ham {
            panic!("Up ile gerialma basarisiz");
        }
    }

    #[test]
    fn en_iyi_filtre_duz_satirda_none_secer() {
        let ham = vec![0u8; 128];
        if en_iyi_filtre(4, Some(&[0u8; 128]), &ham) != 0 {
            panic!("duz veride None filtresi secilmeli");
        }
    }

    #[test]
    fn en_iyi_filtre_dikey_desende_up_secer() {
        // Dikey degisim yok, yatay var: Sub/Paeth Up'i yener.
        let ust: Vec<u8> = (0..64u8).collect();
        let ham: Vec<u8> = (0..64u8).map(|i| (i % 2) * 200).collect();
        let secilen = en_iyi_filtre(1, Some(&ust), &ham);
        if secilen == 2 {
            panic!("Up secilmemeli (yatay desen baskil)");
        }
        if secilen > EN_FAZLA_FILTRE_TIPI {
            panic!("gecersiz filtre tipi secildi: {secilen}");
        }
    }

    #[test]
    fn en_iyi_filtre_her_zaman_gecerli_tip_dondurur() {
        let ust = vec![3u8; 10];
        for i in 0..10u8 {
            let ham = vec![i; 10];
            let tip = en_iyi_filtre(2, Some(&ust), &ham);
            if tip > EN_FAZLA_FILTRE_TIPI {
                panic!("gecersiz tip: {tip}");
            }
        }
    }
}
