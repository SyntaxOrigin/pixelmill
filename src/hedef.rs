//! Hedef bayta ulaşmak için **ikili arama** (rapor bölüm 05, "Hedef boyut
//! arama stratejisi").
//!
//! ## Algoritma (rapordaki sıra korunur)
//!
//! ```text
//! hedef = 120_000 bayt
//!
//! 1) kaba üst sınır denemesi
//!    kalite = 80
//!    boyut  = kodla(kalite)        -> 156_000   (hedefin üstü -> kalite azalt)
//! 2) ikili arama kalite 1..=100 arasında
//!    kalite = 62
//!    boyut  = kodla(kalite)        ->  98_000   (hedefin altı -> kalite artır)
//!    kalite = 71
//!    boyut  = kodla(kalite)        -> 124_000   (hedefin üstü -> kalite azalt)
//! 3) iki ardışık kalite karşılaştırılır; bütçe içinde kalan
//!    EN YÜKSEK kalite seçilir
//! 4) ulaşılamazsa: once cozunurluk, son kalite (asagida)
//! ```
//!
//! Arama `alt` (bütçe içinde kalan en yüksek kalite) ve `ust` (bütçe dışı olduğu
//! bilinen en düşük kalite) sınırları arasında yürür; `ust - alt <= 1` olduğunda
//! durur. `0` ve `101` "henüz bilinmiyor" anlamına gelen nöbet değerlerdir.
//!
//! ## Ulaşılamazsa sıra
//!
//! Rapor: "önce renk derinliği ve kroma alt örneklemesi kısılır, sonra hedefe
//! en yakın kare hızına benzeyen biçimler tercih edilir, en sonunda çözünürlük
//! küçültülür."
//!
//! PNG'de kroma alt örneklemesi ve renk derinliği zaten **kalite kademesi**
//! ile yönetilir ([`crate::kodlama::Kademe`]), dolayısıyla kalite 1'e
//! çekildiğinde renk derinliği en kademedir. Hedeften hâlâ büyükse çözünürlük
//! en-boy oranı korunarak küçültülür. Sonu hâlâ büyükse `Hata::HedefBulunamadi`
//! döner — sıkıştırma oranının libpng'den kötü olması beklenen bir durumdur ve
//! dürüstçe bildirilir (MANIFEST kart 04, "Riskler").

use crate::boyut::Filtre;
use crate::gorsel::Raster;
use crate::hata::Hata;
use crate::kodlama::KodlamaPlani;
use crate::png::yazma::PngYazici;

/// Bir kalite denemesinin ölçümü.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deneme {
    /// Denenen kalite (0..=100).
    pub kalite: u8,
    /// Üretilen PNG'nin bayt cinsinden boyutu.
    pub boyut: u64,
    /// Denemedeki çözünürlük.
    pub cozunurluk: (u32, u32),
    /// Bu deneme hedefin altında kaldı mı?
    pub hedefin_altinda: bool,
}

/// İkili aramanın sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AramaSonucu {
    /// Seçilen kalite.
    pub secilen_kalite: u8,
    /// Seçilen çıktının bayt cinsinden boyutu.
    pub secilen_boyut: u64,
    /// Seçilen çıktının tam PNG baytları.
    pub cikti: Vec<u8>,
    /// Seçilen çıktının çözünürlüğü.
    pub secilen_cozunurluk: (u32, u32),
    /// Yapılan tüm denemeler (rapor için kronolojik).
    pub denemeler: Vec<Deneme>,
    /// Çözünürlük küçültme adımı çalıştırıldı mı?
    pub cozunurluk_kucultuldu: bool,
    /// Kullanıcıya gösterilecek uyarılar.
    pub uyarilar: Vec<String>,
    /// Seçilen çıktının kullanılan biçim planı (rapor için).
    pub plan: KodlamaPlani,
}

/// Bir gövdeyi verilen kalitede PNG olarak kodlar.
///
/// Çıktı doğrudan `Vec<u8>` olarak döner: `PngYazici` her satırı sıkıştırıp
/// yazdığı için kaynak dosya hiçbir zaman tam olarak belleğe alınmaz.
///
/// # Hatalar
///
/// Kalite geçersizse veya yazma hatası olursa hata döner.
pub fn kodla(govde: &Raster, kalite: u8) -> Result<(Vec<u8>, KodlamaPlani), Hata> {
    let plani = KodlamaPlani::olustur(govde, kalite)?;
    let mut cikti = Vec::new();
    {
        let yazici = PngYazici::yeni(&mut cikti, plani.secenek.clone())?;
        let mut w = yazici;
        for y in 0..govde.yukseklik {
            w.yaz(&plani.satir(y))?;
        }
        let _ = w.bitir()?;
    }
    Ok((cikti, plani))
}

/// Bir kalitede kodlanan çıktının bayt cinsinden boyutunu ölçür.
///
/// # Hatalar
///
/// Kodlama hatası olursa hata döner.
pub fn boyut_olc(govde: &Raster, kalite: u8) -> Result<u64, Hata> {
    Ok(kodla(govde, kalite)?.0.len() as u64)
}

/// İkili arama ile hedef bayta sığan **en yüksek kaliteyi** bulur ve çıktıyı
/// üretir.
///
/// `en_fazla_genislik` / `en_fazla_yukseklik` verilirse görüntü önce en-boy
/// oranı korunarak bu sınıra küçültülür (rapordaki "en fazla 1600 piksel" tipi
/// kısıt).
///
/// # Hatalar
///
/// Hedef sıfırsa, kodlama hatası olursa veya hiçbir çözünürlük/kalite
/// kombinasyonu hedefe sığmazsa hata döner.
pub fn hedef_boyut_arar(
    govde: &Raster,
    hedef_bayt: u64,
    en_fazla_genislik: Option<u32>,
    en_fazla_yukseklik: Option<u32>,
    filtre: Filtre,
) -> Result<AramaSonucu, Hata> {
    if hedef_bayt == 0 {
        return Err(Hata::HedefBoyutGecersiz(hedef_bayt));
    }

    let mut calisma = govde.clone();
    if let (Some(g), Some(y)) = (en_fazla_genislik, en_fazla_yukseklik) {
        let (ig_g, ig_y) =
            crate::kip::en_boy_oranli_hedef(govde.genislik, govde.yukseklik, g.max(1), y.max(1));
        if (ig_g, ig_y) != (govde.genislik, govde.yukseklik) {
            calisma = crate::boyut::govde_yeniden_boyutlandir(govde, ig_g, ig_y, filtre)?;
        }
    }

    let mut denemeler: Vec<Deneme> = Vec::new();
    let mut uyarilar: Vec<String> = Vec::new();
    let baslangic_cozunurluk = (calisma.genislik, calisma.yukseklik);

    // `alt`: bütçe içinde olduğu bilinen en yüksek kalite (0 = bilinmiyor).
    // `ust`: bütçe dışı olduğu bilinen en düşük kalite (101 = bilinmiyor).
    let mut alt: u16 = 0;
    let mut ust: u16 = 101;
    let mut en_iyi: Option<(u8, Vec<u8>, u64, KodlamaPlani)> = None;

    // Adım 1: kaba üst sınır denemesi (rapor madde 1).
    let (kaba_bayt, kaba_plani) = kodla(&calisma, 80)?;
    let kaba_boyut = kaba_bayt.len() as u64;
    denemeler.push(Deneme {
        kalite: 80,
        boyut: kaba_boyut,
        cozunurluk: baslangic_cozunurluk,
        hedefin_altinda: kaba_boyut <= hedef_bayt,
    });
    if kaba_boyut <= hedef_bayt {
        alt = 80;
        en_iyi = Some((80, kaba_bayt, kaba_boyut, kaba_plani));
    } else {
        ust = 80;
    }

    // Adım 2: kalite tabanını (1) ölç; hedefin altında değilse çözünürlük küçülür.
    let (taban_bayt, taban_plani) = kodla(&calisma, 1)?;
    let taban_boyut = taban_bayt.len() as u64;
    denemeler.push(Deneme {
        kalite: 1,
        boyut: taban_boyut,
        cozunurluk: baslangic_cozunurluk,
        hedefin_altinda: taban_boyut <= hedef_bayt,
    });
    if taban_boyut <= hedef_bayt {
        if alt == 0 {
            alt = 1;
            en_iyi = Some((1, taban_bayt, taban_boyut, taban_plani));
        }
    } else {
        ust = 1;
        // Adım 3: çözünürlüğü küçült (en-boy oranı korunur).
        let kucultuldu = cozunurluk_kucult(&mut calisma, hedef_bayt, filtre, &mut denemeler);
        if kucultuldu {
            uyarilar.push(format!(
                "hedefe ulasmak icin cozunurluk {}x{} degerine kucultuldu",
                calisma.genislik, calisma.yukseklik
            ));
            let (yeni_bayt, yeni_plani) = kodla(&calisma, 1)?;
            let yeni_boyut = yeni_bayt.len() as u64;
            denemeler.push(Deneme {
                kalite: 1,
                boyut: yeni_boyut,
                cozunurluk: (calisma.genislik, calisma.yukseklik),
                hedefin_altinda: yeni_boyut <= hedef_bayt,
            });
            if yeni_boyut <= hedef_bayt {
                alt = 1;
                en_iyi = Some((1, yeni_bayt, yeni_boyut, yeni_plani));
            } else {
                ust = 1;
            }
        }
    }

    // Adım 4: ikili arama.
    while ust.saturating_sub(alt) > 1 {
        let orta = ((alt + ust) / 2) as u8;
        if orta == 0 || orta == 80 {
            // Aralık zaten daralmış; güvenli dur.
            break;
        }
        let (bayt, plani) = kodla(&calisma, orta)?;
        let boyut = bayt.len() as u64;
        let altinda = boyut <= hedef_bayt;
        denemeler.push(Deneme {
            kalite: orta,
            boyut,
            cozunurluk: (calisma.genislik, calisma.yukseklik),
            hedefin_altinda: altinda,
        });
        if altinda {
            alt = u16::from(orta);
            en_iyi = Some((orta, bayt, boyut, plani));
        } else {
            ust = u16::from(orta);
        }
    }

    match en_iyi {
        Some((kalite, cikti, boyut, plan)) => Ok(AramaSonucu {
            secilen_kalite: kalite,
            secilen_boyut: boyut,
            cikti,
            secilen_cozunurluk: (calisma.genislik, calisma.yukseklik),
            denemeler,
            cozunurluk_kucultuldu: baslangic_cozunurluk != (calisma.genislik, calisma.yukseklik),
            uyarilar,
            plan,
        }),
        None => {
            let en_kucuk = denemeler.iter().map(|d| d.boyut).min().unwrap_or(u64::MAX);
            uyarilar.push(format!(
                "hedef {hedef_bayt} bayta ulasilamadi; en iyi sonuc {en_kucuk} bayt"
            ));
            Err(Hata::HedefBulunamadi {
                hedef: hedef_bayt,
                en_iyi: en_kucuk,
            })
        }
    }
}

/// Kalite 1 bile hedefe sığmıyorsa çözünürlüğü en-boy korunarak küçültür.
///
/// En fazla 10 kez `%87` ile küçültme dener; her küçültmeden sonra kalite 1
/// yeniden ölçülür. Başarılı olursa `true`, olmazsa `false` döner.
fn cozunurluk_kucult(
    govde: &mut Raster,
    hedef_bayt: u64,
    filtre: Filtre,
    denemeler: &mut Vec<Deneme>,
) -> bool {
    for _ in 0..10 {
        let yeni_g = (govde.genislik * 87 / 100).max(1);
        let yeni_y = (govde.yukseklik * 87 / 100).max(1);
        if (yeni_g, yeni_y) == (govde.genislik, govde.yukseklik) {
            return false;
        }
        let kucultulmus = crate::boyut::govde_yeniden_boyutlandir(govde, yeni_g, yeni_y, filtre);
        match kucultulmus {
            Ok(yeni) => {
                *govde = yeni;
                let boyut = match boyut_olc(govde, 1) {
                    Ok(b) => b,
                    Err(_) => return false,
                };
                denemeler.push(Deneme {
                    kalite: 1,
                    boyut,
                    cozunurluk: (govde.genislik, govde.yukseklik),
                    hedefin_altinda: boyut <= hedef_bayt,
                });
                if boyut <= hedef_bayt {
                    return true;
                }
            }
            Err(_) => return false,
        }
    }
    false
}
