//! Uçtan uca entegrasyon testleri: gerçek dosya sistemi üzerinde PNG gidiş-dönüş,
//! yeniden boyutlandırma, kırpma, kuru çalıştırma ve toplu kuyruk davranışı.
//!
//! Testler deterministiktir: rastgelelik crate'i kullanılmaz, piksel deseni
//! sabit tohumlu LCG ile üretilir ve geçici dizin `std::env::temp_dir()` altında
//! `std::process::id()` ile benzersizleştirilir.

use std::path::{Path, PathBuf};
use std::process::Command;

use pixelmill::boyut::Filtre;
use pixelmill::gorsel::Raster;
use pixelmill::hata::Hata;
use pixelmill::islem::{dosyayi_isle, klasoru_isle, IslemAyar};
use pixelmill::kuyruk::{kuyrugu_olustur, GezintiSecenekleri, KuyrukGirdisi};
use pixelmill::png::yazma::{png_kodla, PngSecenek};
use pixelmill::Rapor;

/// Test içinde geçici dosya/dizin üreten, `Drop` ile temizleyen kapsayıcı.
///
/// Neden `tempfile` yok: bağımlılık politikası (WORKER_CONTRACT § 3.2) `tempfile`'i
/// hiçbir projede vermez; yardımcı kendi kodumuzla yazılır.
pub struct GeciciDizin {
    yol: PathBuf,
}

impl GeciciDizin {
    /// `std::env::temp_dir()` altında etiketten türetilmiş benzersiz dizin üretir.
    ///
    /// # Panik
    ///
    /// Dizin oluşturulamazsa panikler; test zaten bu noktadan sonra çalışamaz.
    pub fn yeni(etiket: &str) -> Self {
        let kok = std::env::temp_dir().join(format!("pixelmill-{etiket}-{}", std::process::id()));
        // Aynı testin iki kez çalışması olası; eski içerik temizlenir.
        let _ = std::fs::remove_dir_all(&kok);
        std::fs::create_dir_all(&kok).expect("gecici dizin olusturulamadi");
        Self { yol: kok }
    }

    /// Dizin içine göreli yol döndürür.
    pub fn yol(&self) -> &Path {
        &self.yol
    }

    /// Dizin altına yol birleştirir.
    pub fn birlestir(&self, ad: &str) -> PathBuf {
        self.yol.join(ad)
    }
}

impl Drop for GeciciDizin {
    fn drop(&mut self) {
        // Temizlik başarısız olsa da testi düşürmemeli; `let _ =` bilinçlidir
        // (Drop içinden hata döndürülemez).
        let _ = std::fs::remove_dir_all(&self.yol);
    }
}

/// `Result`'i `Debug` çıktısıyla panik mesajına çevirir (clippy::unwrap_used yok).
fn ac<T, E: std::fmt::Debug>(sonuc: Result<T, E>, baglam: &str) -> T {
    match sonuc {
        Ok(deger) => deger,
        Err(hata) => panic!("{baglam}: beklenmeyen hata {hata:?}"),
    }
}

/// Sabit tohumlu LCG ile gürültü üretir (rastgelelik crate'i yok).
fn gurultu(tohum: &mut u32) -> u8 {
    *tohum = tohum.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*tohum >> 16) as u8
}

/// Testlerde kullanılan tam renkli PNG'yi diske yazar.
fn png_yaz(yol: &Path, genislik: u32, yukseklik: u32, tohum: u32) {
    let mut g = ac(Raster::yeni_sifir(genislik, yukseklik), "raster");
    let mut t = tohum;
    for y in 0..yukseklik {
        for x in 0..genislik {
            g.piksel_ata(
                x,
                y,
                [gurultu(&mut t), gurultu(&mut t), gurultu(&mut t), 255],
            );
        }
    }
    let secenek = ac(PngSecenek::yeni(genislik, yukseklik, 8, 6), "secenek");
    let baytlar = ac(png_kodla(&secenek, &mut |y| g.satir(y)), "kodlama");
    std::fs::write(yol, baytlar).expect("png yazilamadi");
}

/// `pixelmill` ikilisinin mutlak yolunu bulur.
fn ikili_yolu() -> PathBuf {
    // `CARGO_BIN_EXE_<ad>` yalnızca entegrasyon testlerinde tanımlıdır.
    PathBuf::from(env!("CARGO_BIN_EXE_pixelmill"))
}

#[test]
fn png_dosya_gidis_donus_basar() {
    let gecici = GeciciDizin::yeni("gidis-donus");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 24, 16, 0x00C0_FFEE);

    let cikti = gecici.birlestir("cikti.png");
    // Kalite 100 `TamRenk` kademesidir (bkz. `kodlama::Kademe::kaliteden`);
    // varsayilan 80 `Palet8` kademesidir ve renk kuantizasyonu nedeniyle
    // kasitli olarak kayiptir. Gidis-donusu piksel bazinda karsilastirmak icin
    // kayipsiz kademe secilir.
    let ayar = IslemAyar {
        sabit_kalite: Some(100),
        ..IslemAyar::default()
    };
    let rapor = ac(dosyayi_isle(&kaynak, &cikti, &ayar, false), "islem");
    assert!(rapor.basarili, "islem basarisiz: {:?}", rapor.hata_mesaji);
    assert!(cikti.exists(), "cikti dosyasi olusmadi");

    let okunan = ac(pixelmill::png::png_dosya_coz(&cikti), "cozme");
    let ilk = ac(pixelmill::png::png_dosya_coz(&kaynak), "kaynak cozme");
    assert_eq!(okunan.baslik.genislik, 24);
    assert_eq!(okunan.baslik.yukseklik, 16);
    assert_eq!(okunan.govde.piksel, ilk.govde.piksel, "gidis-donus farkli");
}

#[test]
fn dry_run_hicbir_dosya_yazmaz() {
    let gecici = GeciciDizin::yeni("dry-run");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 12, 12, 0x1111_2222);
    let cikti = gecici.birlestir("cikti.png");

    let ayar = IslemAyar::default();
    let rapor = ac(
        dosyayi_isle(&kaynak, &cikti, &ayar, true),
        "kuru calistirma",
    );
    assert!(rapor.basarili);
    assert!(!cikti.exists(), "kuru calistirma cikti dosyasi uretmemeli");
}

#[test]
fn cikti_kaynaga_ustune_yazmaz() {
    let gecici = GeciciDizin::yeni("cakisma");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 8, 8, 0x0BAD_F00D);
    let onceki = std::fs::read(&kaynak).expect("okuma");

    let ayar = IslemAyar::default();
    match dosyayi_isle(&kaynak, &kaynak, &ayar, false) {
        Err(Hata::CiktiCakismasi(_)) => {}
        diger => panic!("cakisma reddedilmeli, alinan: {diger:?}"),
    }
    let sonra = std::fs::read(&kaynak).expect("okuma");
    assert_eq!(onceki, sonra, "kaynak dosya degistirilmemeli");
}

#[test]
fn yeniden_boyutlandirma_tam_olcu_verir() {
    let gecici = GeciciDizin::yeni("boyut");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 40, 20, 0x7777_8888);
    let cikti = gecici.birlestir("kucuk.png");

    let ayar = IslemAyar {
        hedef_genislik: Some(20),
        hedef_yukseklik: Some(10),
        filtre: Filtre::Bicubic,
        ..IslemAyar::default()
    };
    let rapor = ac(dosyayi_isle(&kaynak, &cikti, &ayar, false), "islem");
    assert!(rapor.basarili, "{}", rapor.ozet());

    let okunan = ac(pixelmill::png::png_dosya_coz(&cikti), "cozme");
    assert_eq!(okunan.baslik.genislik, 20);
    assert_eq!(okunan.baslik.yukseklik, 10);
}

#[test]
fn en_fazla_genislik_tek_basina_kucultur() {
    // Regresyon: `--en-fazla-genislik` tek basina verildiginde ozluluk hic
    // uygulanmiyordu (ikinci eksen `None` oldugu icin dallanma hic girmiyordu).
    let gecici = GeciciDizin::yeni("en-fazla");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 200, 150, 0xC0FF_EE01);
    let cikti = gecici.birlestir("dar.png");

    let ayar = IslemAyar {
        sabit_kalite: Some(100),
        en_fazla_genislik: Some(100),
        filtre: Filtre::Kutu,
        ..IslemAyar::default()
    };
    let rapor = ac(dosyayi_isle(&kaynak, &cikti, &ayar, false), "islem");
    assert!(rapor.basarili, "{}", rapor.ozet());

    let okunan = ac(pixelmill::png::png_dosya_coz(&cikti), "cozme");
    assert_eq!(okunan.baslik.genislik, 100, "genislik sinirina sigmali");
    // 200x150 -> 100x75 (en-boy orani korunur).
    assert_eq!(okunan.baslik.yukseklik, 75, "en-boy orani korunmali");
}

#[test]
fn en_fazla_genislik_gorseli_buyutmez() {
    // 40x30 gorseli icin 200 piksel siniri: gorselt buyutulmemeli.
    let gecici = GeciciDizin::yeni("en-fazla-buyutme");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 40, 30, 0xFEED_FACE);
    let cikti = gecici.birlestir("cikti.png");

    let ayar = IslemAyar {
        sabit_kalite: Some(100),
        en_fazla_genislik: Some(200),
        ..IslemAyar::default()
    };
    let rapor = ac(dosyayi_isle(&kaynak, &cikti, &ayar, false), "islem");
    assert!(rapor.basarili, "{}", rapor.ozet());

    let okunan = ac(pixelmill::png::png_dosya_coz(&cikti), "cozme");
    assert_eq!(okunan.baslik.genislik, 40, "buyutulmemeli");
    assert_eq!(okunan.baslik.yukseklik, 30, "buyutulmemeli");
}

#[test]
fn kirpma_istenen_pencerayi_verir() {
    let gecici = GeciciDizin::yeni("kirpma");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 30, 30, 0x1357_9BDF);
    let cikti = gecici.birlestir("kirp.png");

    let ayar = IslemAyar {
        sabit_kalite: Some(100),
        kirpma: Some((5, 6, 8, 8)),
        ..IslemAyar::default()
    };
    let rapor = ac(dosyayi_isle(&kaynak, &cikti, &ayar, false), "islem");
    assert!(rapor.basarili, "{}", rapor.ozet());

    let okunan = ac(pixelmill::png::png_dosya_coz(&cikti), "cozme");
    assert_eq!(okunan.baslik.genislik, 8);
    assert_eq!(okunan.baslik.yukseklik, 8);
    // Kirpilan piksel kaynagin ayni konumundaki pikseliyle eslesmelidir.
    let kaynak_icerik = ac(pixelmill::png::png_dosya_coz(&kaynak), "kaynak cozme");
    assert_eq!(okunan.govde.piksel(0, 0), kaynak_icerik.govde.piksel(5, 6));
}

#[test]
fn hedef_boyut_arama_butceye_uyar() {
    let gecici = GeciciDizin::yeni("hedef");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 60, 60, 0x2468_ACE0);
    let cikti = gecici.birlestir("hedef.png");
    let hedef: u64 = 4_000;

    let ayar = IslemAyar {
        sabit_kalite: None,
        hedef_bayt: Some(hedef),
        ..IslemAyar::default()
    };
    let rapor = ac(dosyayi_isle(&kaynak, &cikti, &ayar, false), "islem");
    assert!(rapor.basarili, "{}", rapor.ozet());

    let boyut = std::fs::metadata(&cikti).expect("cikti yok").len();
    assert!(
        boyut <= hedef,
        "hedef {hedef} asildi: {boyut} bayt uretildi"
    );
}

#[test]
fn toplu_kuyruk_bir_hata_digerini_durdurmaz() {
    let gecici = GeciciDizin::yeni("toplu");
    let kaynak = gecici.birlestir("kaynaklar");
    std::fs::create_dir_all(&kaynak).expect("dizin");
    png_yaz(&kaynak.join("a.png"), 16, 16, 0x1111_1111);
    png_yaz(&kaynak.join("b.png"), 16, 16, 0x2222_2222);
    // Bozuk dosya: imza geçerli ama govde okunamaz.
    std::fs::write(kaynak.join("bozuk.png"), b"\x89PNG\r\n\x1a\nBOZUK").expect("yaz");
    // Desteklenmeyen uzantı kuyruğa girmemeli.
    std::fs::write(kaynak.join("notlar.txt"), b"merhaba").expect("yaz");
    // Gizli dosya atlanmalı.
    png_yaz(&kaynak.join(".gizli.png"), 16, 16, 0x3333_3333);

    let cikti = gecici.birlestir("ciktilar");
    let ayar = IslemAyar::default();
    let rapor = ac(klasoru_isle(&kaynak, &cikti, &ayar, false), "toplu");

    assert!(
        cikti.join("a.png").exists() && cikti.join("b.png").exists(),
        "gecerli dosyalar islenmeli"
    );
    assert!(
        !cikti.join("bozuk.png").exists(),
        "bozuk dosya cikti uretmemeli"
    );
    assert!(
        !cikti.join("notlar.txt").exists(),
        "desteklenmeyen uzantı islenmemeli"
    );
    assert!(
        !cikti.join(".gizli.png").exists(),
        "gizli dosya varsayilan olarak atlanmali"
    );
    // Tek bir bozuk dosya kuyrugu durdurmamali.
    assert_eq!(rapor.dosyalar.len(), 3, "rapor satiri: {}", rapor.metin());
    assert!(
        rapor.basarili_dosya >= 2,
        "en az iki dosya basarili olmali: {}",
        rapor.metin()
    );
}

#[test]
fn gizliler_dahil_secenegi_dosyayi_alir() {
    let gecici = GeciciDizin::yeni("gizli");
    let kaynak = gecici.birlestir("kaynaklar");
    std::fs::create_dir_all(&kaynak).expect("dizin");
    png_yaz(&kaynak.join("gorunur.png"), 8, 8, 0x4444_4444);
    png_yaz(&kaynak.join(".gizli.png"), 8, 8, 0x5555_5555);

    let secenek = GezintiSecenekleri {
        atlanacak_dizin: None,
        gizlilere_dahil: true,
    };
    let girdiler = ac(kuyrugu_olustur(&kaynak, &secenek), "kuyruk");
    assert_eq!(girdiler.len(), 2, "gizli dahil edildiginde ikisi de olmali");

    let secenek = GezintiSecenekleri::default();
    let girdiler = ac(kuyrugu_olustur(&kaynak, &secenek), "kuyruk");
    assert_eq!(girdiler.len(), 1, "gizli varsayilan olarak atlanmali");
}

#[test]
fn hedef_boyut_sifiri_reddedilir() {
    let gecici = GeciciDizin::yeni("hedef-sifir");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 8, 8, 0x6666_6666);
    let cikti = gecici.birlestir("cikti.png");

    let ayar = IslemAyar {
        sabit_kalite: None,
        hedef_bayt: Some(0),
        ..IslemAyar::default()
    };
    match dosyayi_isle(&kaynak, &cikti, &ayar, false) {
        Err(Hata::HedefBoyutGecersiz(0)) => {}
        diger => panic!("sifir hedef reddedilmeli, alinan: {diger:?}"),
    }
}

#[test]
fn json_rapor_serialize_edilebilir() {
    let gecici = GeciciDizin::yeni("rapor");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 12, 10, 0x9999_AAAA);
    let cikti = gecici.birlestir("cikti.png");

    let ayar = IslemAyar::default();
    let tek = ac(dosyayi_isle(&kaynak, &cikti, &ayar, false), "islem");
    let mut rapor = Rapor::yeni(false);
    rapor.ekle(tek);
    let json = ac(rapor.json(), "json");
    let deger: serde_json::Value = ac(serde_json::from_str(&json), "json ayristirma");
    assert!(deger.get("dosyalar").is_some(), "json semasi eksik: {json}");
}

#[test]
fn cli_info_goruntu_dogru_bildirir() {
    let gecici = GeciciDizin::yeni("cli-info");
    let kaynak = gecici.birlestir("kaynak.png");
    png_yaz(&kaynak, 20, 10, 0xABCD_0123);

    let cikti = Command::new(ikili_yolu())
        .arg("info")
        .arg(&kaynak)
        .output()
        .expect("calistirilamadi");
    assert!(cikti.status.success(), "info basarisiz: {cikti:?}");
    let metin = String::from_utf8_lossy(&cikti.stdout);
    assert!(metin.contains("20"), "genislik raporda yok: {metin}");
    assert!(metin.contains("10"), "yukseklik raporda yok: {metin}");
}

#[test]
fn cli_bozuk_dosyada_hata_kodu_dondurur() {
    let gecici = GeciciDizin::yeni("cli-bozuk");
    let kaynak = gecici.birlestir("bozuk.png");
    std::fs::write(&kaynak, b"bu bir png degil").expect("yaz");

    let cikti = Command::new(ikili_yolu())
        .arg("info")
        .arg(&kaynak)
        .output()
        .expect("calistirilamadi");
    assert!(!cikti.status.success(), "bozuk dosya hata kodu donmeli");
}

#[test]
fn cli_batch_kuru_calistirma_dosya_yazmaz() {
    let gecici = GeciciDizin::yeni("cli-batch");
    let kaynak = gecici.birlestir("kaynaklar");
    std::fs::create_dir_all(&kaynak).expect("dizin");
    png_yaz(&kaynak.join("bir.png"), 10, 10, 0xAAAA_AAAA);
    let cikti = gecici.birlestir("cikti");

    let sonuc = Command::new(ikili_yolu())
        .arg("batch")
        .arg(&kaynak)
        .arg("--cikti-dizini")
        .arg(&cikti)
        .arg("--kuru-calistir")
        .output()
        .expect("calistirilamadi");
    assert!(sonuc.status.success(), "batch basarisiz: {sonuc:?}");
    assert!(
        !cikti.join("bir.png").exists(),
        "kuru calistirma dosya yazmamali"
    );
}

#[test]
fn kuyruk_girdisi_dosya_adini_dogru_verir() {
    let girdi = KuyrukGirdisi::yeni(PathBuf::from("/a/b/ornek.png"), 42);
    assert_eq!(girdi.dosya_adi(), "ornek.png");
    assert_eq!(girdi.boyut, 42);
}
