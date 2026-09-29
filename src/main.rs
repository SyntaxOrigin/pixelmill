//! PixelMill komut satırı giriş noktası.
//!
//! Bu dosya yalnızca **kabuktur**: argüman ayrıştırma, kitaplık çağrıları ve
//! çıktı yazımı. Tüm iş mantığı `pixelmill` kütüphanesindedir (`src/lib.rs`),
//! böylece testler ikiliyi derlemeden çalışabilir.
//!
//! ## Çıkış kodları
//!
//! | Kod | Anlamı |
//! |-----|--------|
//! | 0  | başarılı (raporda hata satırı olsa da toplu iş tamamlandıysa) |
//! | 1  | kullanım/iş hatası (ayrıntı `stderr`'e yazılır) |
//! | 2  | toplu iş tamamlandı ama en az bir dosya başarısız oldu |

#![forbid(unsafe_code)]

use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use pixelmill::cli::{BatchArg, Cli, InfoArg, KaynakCiktiArg, Komut, ResizeArg};
use pixelmill::hata::{io_hata, Hata};
use pixelmill::islem::{klasoru_isle, Bicim, IslemAyar};
use pixelmill::rapor::Rapor;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match calistir(cli) {
        Ok(kod) => kod,
        Err(hata) => {
            eprintln!("pixelmill: {hata}");
            ExitCode::from(1)
        }
    }
}

/// Komutu çalıştırır ve istenen çıkış kodunu döndürür.
///
/// # Hatalar
///
/// Argüman geçersizse veya tekil dosya işlemi başarısız olursa hata döner.
fn calistir(cli: Cli) -> Result<ExitCode, Hata> {
    match cli.komut {
        Komut::Info(arg) => info_calistir(&arg),
        Komut::Convert(arg) | Komut::Optimize(arg) => tekil_calistir(&arg),
        Komut::Resize(arg) => resize_calistir(&arg),
        Komut::Batch(arg) => batch_calistir(&arg),
    }
}

/// `info` alt komutunu çalıştırır.
///
/// # Hatalar
///
/// Dosya okunamazsa veya biçim tanınmazsa hata döner.
fn info_calistir(arg: &InfoArg) -> Result<ExitCode, Hata> {
    let bicim = pixelmill::islem::bicim_tespit_et(&arg.dosya)?;
    match bicim {
        Bicim::Png => {
            let bilgi = pixelmill::png::png_bilgi(
                std::fs::File::open(&arg.dosya).map_err(|e| io_hata(&arg.dosya, e))?,
            )?;
            if arg.json {
                let metin = serde_json::to_string_pretty(&serde_json::json!({
                    "dosya": arg.dosya.display().to_string(),
                    "bicim": "png",
                    "genislik": bilgi.baslik.genislik,
                    "yukseklik": bilgi.baslik.yukseklik,
                    "bit_derinligi": bilgi.baslik.bit_derinligi,
                    "renk_tipi": bilgi.baslik.renk_tipi,
                    "renk_tipi_adi": bilgi.baslik.renk_tipi_adi(),
                    "palet_girisi": bilgi.palet_uzunlugu,
                    "sekme": bilgi.baslik.sekme,
                    "bloklar": bilgi.bloklar,
                    "metin": bilgi.metin.iter().map(|m| format!("{}={}", m.anahtar, m.deger)).collect::<Vec<String>>(),
                    "exif_var": bilgi.exif.is_some(),
                }))
                .map_err(|e| Hata::RaporHatasi(e.to_string()))?;
                println!("{metin}");
            } else {
                println!("dosya      : {}", arg.dosya.display());
                println!("bicim      : PNG");
                println!(
                    "boyut      : {}x{}",
                    bilgi.baslik.genislik, bilgi.baslik.yukseklik
                );
                println!(
                    "renk tipi  : {} ({})",
                    bilgi.baslik.renk_tipi_adi(),
                    bilgi.baslik.renk_tipi
                );
                println!("bit derin. : {}", bilgi.baslik.bit_derinligi);
                println!("palet      : {} giris", bilgi.palet_uzunlugu);
                println!("sekme      : {}", bilgi.baslik.sekme);
                println!("bloklar    : {}", bilgi.bloklar.join(", "));
                for m in &bilgi.metin {
                    println!("metin      : {} = {}", m.anahtar, m.deger);
                }
                println!(
                    "exif       : {}",
                    if bilgi.exif.is_some() { "var" } else { "yok" }
                );
            }
        }
        Bicim::Jpeg => {
            let baslik = pixelmill::jpeg::jpeg_dosya_basligi(&arg.dosya)?;
            if arg.json {
                let metin = serde_json::to_string_pretty(&serde_json::json!({
                    "dosya": arg.dosya.display().to_string(),
                    "bicim": "jpeg",
                    "genislik": baslik.cerceve.as_ref().map(|c| c.genislik),
                    "yukseklik": baslik.cerceve.as_ref().map(|c| c.yukseklik),
                    "bilesenler": baslik.cerceve.as_ref().map(|c| c.bilesenler.len()).unwrap_or(0),
                    "kuantizasyon_tablolari": baslik.kuantizasyon.len(),
                    "huffman_tablolari": baslik.huffman_tablo_sayisi,
                    "app_etiketleri": baslik.app_etiketleri,
                    "jfif_surumu": baslik.jfif_surumu,
                    "exif_var": baslik.exif.is_some(),
                    "markerlar": baslik.markerlar,
                }))
                .map_err(|e| Hata::RaporHatasi(e.to_string()))?;
                println!("{metin}");
            } else {
                println!("dosya      : {}", arg.dosya.display());
                println!("bicim      : JPEG");
                if let Some(c) = &baslik.cerceve {
                    println!("boyut      : {}x{}", c.genislik, c.yukseklik);
                    println!(
                        "bilesen    : {} (en yuksek ornek carpimi {})",
                        c.bilesen_sayisi,
                        c.en_yuksek_carp()
                    );
                    for b in &c.bilesenler {
                        println!(
                            "  bilesen {} : {}x{} kuantizasyon tablosu {}",
                            b.kimlik, b.yatay, b.dikey, b.kuantizasyon
                        );
                    }
                }
                println!("kuantizasyon tablolari: {}", baslik.kuantizasyon.len());
                for t in &baslik.kuantizasyon {
                    println!(
                        "  tablo {} : {} bit, DC carpani {}",
                        t.kimlik,
                        t.hassasiyet(),
                        t.dc_carpan()
                    );
                }
                println!("huffman tablolari    : {}", baslik.huffman_tablo_sayisi);
                println!("APP segmentleri      : {:?}", baslik.app_etiketleri);
                println!("JFIF surumu          : {:?}", baslik.jfif_surumu);
                println!(
                    "exif                 : {}",
                    if baslik.exif.is_some() { "var" } else { "yok" }
                );
                println!("markerlar            : {}", baslik.markerlar.join(", "));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `convert` ve `optimize` alt komutlarını çalıştırır.
///
/// # Hatalar
///
/// Ayarlar geçersizse veya dosya işlenemezse hata döner.
fn tekil_calistir(arg: &KaynakCiktiArg) -> Result<ExitCode, Hata> {
    let ayar = arg.ayara()?;
    let dosya = pixelmill::islem::dosyayi_isle(&arg.girdi, &arg.cikti, &ayar, arg.kuru_calistir)?;
    let mut rapor = Rapor::yeni(arg.kuru_calistir);
    rapor.ekle(dosya);
    cikti_yaz(&rapor, arg.rapor.as_deref(), arg.json)?;
    if rapor.basarisiz_dosya == 0 {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(2))
    }
}

/// `resize` alt komutunu çalıştırır.
///
/// # Hatalar
///
/// Ayarlar geçersizse veya dosya işlenemezse hata döner.
fn resize_calistir(arg: &ResizeArg) -> Result<ExitCode, Hata> {
    let ayar: IslemAyar = arg.ayara()?;
    let dosya = pixelmill::islem::dosyayi_isle(&arg.girdi, &arg.cikti, &ayar, arg.kuru_calistir)?;
    let mut rapor = Rapor::yeni(arg.kuru_calistir);
    rapor.ekle(dosya);
    cikti_yaz(&rapor, arg.rapor.as_deref(), arg.json)?;
    if rapor.basarisiz_dosya == 0 {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(2))
    }
}

/// `batch` alt komutunu çalıştırır.
///
/// # Hatalar
///
/// Kaynak okunamazsa veya çıktı dizini oluşturulamazsa hata döner.
fn batch_calistir(arg: &BatchArg) -> Result<ExitCode, Hata> {
    let ayar = arg.ayara()?;
    let rapor = klasoru_isle(&arg.kaynak, &arg.cikti_dizini, &ayar, arg.kuru_calistir)?;
    cikti_yaz(&rapor, arg.rapor.as_deref(), arg.json)?;
    if rapor.basarisiz_dosya == 0 {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(2))
    }
}

/// Raporu ekrana ve istenirse dosyaya yazar.
///
/// # Hatalar
///
/// Rapor dosyası yazılamazsa veya serileştirilemezse hata döner.
fn cikti_yaz(rapor: &Rapor, dosya: Option<&Path>, json: bool) -> Result<(), Hata> {
    if json {
        println!("{}", rapor.json()?);
    } else {
        println!("{}", rapor.metin());
    }
    if let Some(yol) = dosya {
        std::fs::write(yol, format!("{}\n", rapor.json()?)).map_err(|e| io_hata(yol, e))?;
    }
    Ok(())
}
