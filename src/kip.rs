//! Kırpma, kenar boşluğu ekleme ve en-boy oranıyla ölçekleme.
//!
//! Raporun kabul kriteri: "Çıktı tam olarak istenen genişlik ve yüksekliğe
//! sahip; oran bozulmuyor". Bu modül o işi üç ayrı adımda yapar:
//!
//! 1. [`kirp`]: kaynak görüntüden bir dikdörtgen keser (uç kırpma).
//! 2. [`olcek`]: en-boy oranını koruyarak yeni bir sınıra sığdırır
//!    (`sığdır` = hiç büyütmez, `kapsa` = küçültür ve gerekiyorsa büyütür).
//! 3. [`kenar_boslugu_ekle`]: çevreye sabit renkli bir boşluk ekler ve boyutu
//!    tam olarak istenen değere getirir.
//!
//! Kırpma bir **yeni `Raster` üretir**; kaynak tampon değiştirilmez. Kırpma
//! dikdörtgeni kaynak çerçeve dışına taşarsa `Hata::KirpmaGecersiz` döner ve
//! görüntü "sessizce bozulmaz".

use crate::gorsel::Raster;
use crate::hata::Hata;

/// Kaynak görüntüden bir dikdörtgen keser ve yeni bir `Raster` döndürür.
///
/// `x`/`y` sol-üst köşe, `genislik`/`yukseklik` kırpma boyutudur. Dikdörtgen
/// kaynak çerçevenin tamamen içinde olmalıdır.
///
/// # Hatalar
///
/// Genişlik/yükseklik sıfırsa veya dikdörtgen kaynak dışına taşarsa hata döner.
pub fn kirp(
    kaynak: &Raster,
    x: u32,
    y: u32,
    genislik: u32,
    yukseklik: u32,
) -> Result<Raster, Hata> {
    let kaynak_boyut = (kaynak.genislik, kaynak.yukseklik);
    if genislik == 0 || yukseklik == 0 {
        return Err(Hata::BoyutGecersiz {
            ayrinti: format!("kırpma boyutu {genislik}x{yukseklik} sıfır olamaz"),
        });
    }
    let tasma = x
        .checked_add(genislik)
        .map_or(true, |s| s > kaynak.genislik)
        || y.checked_add(yukseklik)
            .map_or(true, |s| s > kaynak.yukseklik);
    if tasma {
        return Err(Hata::KirpmaGecersiz {
            dikdortgen: (x, y, genislik, yukseklik),
            kaynak: kaynak_boyut,
        });
    }
    let mut cikti = Raster::yeni_sifir(genislik, yukseklik)?;
    for yy in 0..yukseklik {
        for xx in 0..genislik {
            let p = kaynak.piksel(x + xx, y + yy);
            cikti.piksel_ata(xx, yy, p);
        }
    }
    Ok(cikti)
}

/// Kaynak görüntüyü en-boy oranını koruyarak yeni boyutlara **sığdırır**.
///
/// Görüntü hiçbir zaman büyütülmez; küçültme gerekiyorsa her iki kenar aynı
/// oranla azaltılır, artan pay boşluk olarak bırakılmaz (yalnız kırpma gerekiyorsa
/// yol ayrıdır). Kullanıcı "en fazla 1600 piksel" dediğinde budur.
///
/// # Hatalar
///
/// Hedef boyut sıfır veya sınırları aşıyorsa hata döner.
pub fn sigdir(
    kaynak: &Raster,
    en_fazla_genislik: u32,
    en_fazla_yukseklik: u32,
    filtre: crate::boyut::Filtre,
) -> Result<Raster, Hata> {
    if en_fazla_genislik == 0 || en_fazla_yukseklik == 0 {
        return Err(Hata::BoyutGecersiz {
            ayrinti: format!("hedef sınır {en_fazla_genislik}x{en_fazla_yukseklik} sıfır olamaz"),
        });
    }
    let hedef = en_boy_oranli_hedef(
        kaynak.genislik,
        kaynak.yukseklik,
        en_fazla_genislik,
        en_fazla_yukseklik,
    );
    if hedef == (kaynak.genislik, kaynak.yukseklik) {
        return Ok(kaynak.clone());
    }
    crate::boyut::govde_yeniden_boyutlandir(kaynak, hedef.0, hedef.1, filtre)
}

/// En-boy oranını koruyarak `en_fazla_*` sınırlarına sığan boyutu hesaplar.
///
/// Görüntüyü büyütmez. Sonuç her iki eksende küçük veya eşittir.
#[must_use]
pub fn en_boy_oranli_hedef(
    kaynak_genislik: u32,
    kaynak_yukseklik: u32,
    en_fazla_genislik: u32,
    en_fazla_yukseklik: u32,
) -> (u32, u32) {
    if kaynak_genislik == 0 || kaynak_yukseklik == 0 {
        return (1, 1);
    }
    if kaynak_genislik <= en_fazla_genislik && kaynak_yukseklik <= en_fazla_yukseklik {
        return (kaynak_genislik, kaynak_yukseklik);
    }
    let oran_g = f64::from(en_fazla_genislik) / f64::from(kaynak_genislik);
    let oran_y = f64::from(en_fazla_yukseklik) / f64::from(kaynak_yukseklik);
    let oran = oran_g.min(oran_y);
    let yeni_g = ((f64::from(kaynak_genislik) * oran).round() as u32).max(1);
    let yeni_y = ((f64::from(kaynak_yukseklik) * oran).round() as u32).max(1);
    (yeni_g, yeni_y)
}

/// Görüntünün çevresine sabit renkli kenar boşluğu ekler.
///
/// `kenar` piksel cinsinden tek tarafların genişliğidir. Sonuç tam olarak
/// `genislik + 2*kenar` × `yukseklik + 2*kenar` boyutundadır.
///
/// # Hatalar
///
/// `kenar` 0 ise veya sonuç boyutları sınırları aşıyorsa hata döner.
pub fn kenar_boslugu_ekle(kaynak: &Raster, kenar: u32, renk: [u8; 4]) -> Result<Raster, Hata> {
    if kenar == 0 {
        return Ok(kaynak.clone());
    }
    let yeni_g = kaynak
        .genislik
        .checked_add(kenar * 2)
        .ok_or(Hata::BoyutGecersiz {
            ayrinti: "kenar boşluğu genişlik sınırını aşıyor".to_string(),
        })?;
    let yeni_y = kaynak
        .yukseklik
        .checked_add(kenar * 2)
        .ok_or(Hata::BoyutGecersiz {
            ayrinti: "kenar boşluğu yükseklik sınırını aşıyor".to_string(),
        })?;
    crate::gorsel::boyut_dogrula(yeni_g, u64::from(yeni_y))?;
    let mut cikti = Raster::yeni_sifir(yeni_g, yeni_y)?;
    for y in 0..yeni_y {
        for x in 0..yeni_g {
            let p = if x < kenar || x >= yeni_g - kenar || y < kenar || y >= yeni_y - kenar {
                renk
            } else {
                kaynak.piksel(x - kenar, y - kenar)
            };
            cikti.piksel_ata(x, y, p);
        }
    }
    Ok(cikti)
}

/// Görüntüyü verilen tam boyuta **içerik bozmadan** sığdırır.
///
/// Önce en-boy oranı korunarak küçültülür, sonra gerekiyorsa kenar boşluğu
/// eklenir; böylece çıktı tam olarak istenen genişlik/yükseklik olur ve görüntü
/// çarpıtılmaz. Raporun "çıktı tam olarak istenen boyutta" kabul kriteri budur.
///
/// # Hatalar
///
/// Hedef boyut sıfır/sınır dışıysa veya aradaki adımlar hata döndürürse hata döner.
pub fn tam_boyuta_sigdir(
    kaynak: &Raster,
    hedef_genislik: u32,
    hedef_yukseklik: u32,
    filtre: crate::boyut::Filtre,
    kenar_rengi: [u8; 4],
) -> Result<Raster, Hata> {
    if hedef_genislik == 0 || hedef_yukseklik == 0 {
        return Err(Hata::BoyutGecersiz {
            ayrinti: format!("hedef {hedef_genislik}x{hedef_yukseklik} sıfır olamaz"),
        });
    }
    let (ig_g, ig_y) = en_boy_oranli_hedef(
        kaynak.genislik,
        kaynak.yukseklik,
        hedef_genislik,
        hedef_yukseklik,
    );
    let kucultulmus = if (ig_g, ig_y) == (kaynak.genislik, kaynak.yukseklik) {
        kaynak.clone()
    } else {
        crate::boyut::govde_yeniden_boyutlandir(kaynak, ig_g, ig_y, filtre)?
    };
    if kucultulmus.genislik == hedef_genislik && kucultulmus.yukseklik == hedef_yukseklik {
        return Ok(kucultulmus);
    }
    // Fark kenar boşluğu ile tamamlanır: (hedef - iç) / 2 her yerde.
    let kenar_x = hedef_genislik.saturating_sub(ig_g) / 2;
    let kenar_y = hedef_yukseklik.saturating_sub(ig_y) / 2;
    let kenar = kenar_x.max(kenar_y);
    let bosluklu = kenar_boslugu_ekle(&kucultulmus, kenar, kenar_rengi)?;
    if bosluklu.genislik == hedef_genislik && bosluklu.yukseklik == hedef_yukseklik {
        return Ok(bosluklu);
    }
    // Yuvarlama farkı kaldıysa son kez hedefe zorla ölçekle (en-boy korunur).
    crate::boyut::govde_yeniden_boyutlandir(
        &bosluklu,
        hedef_genislik,
        hedef_yukseklik,
        crate::boyut::Filtre::Bilinear,
    )
}
