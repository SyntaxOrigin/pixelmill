# PixelMill (PikselAtölyesi)

Toplu PNG görsel dönüştürücü, yeniden boyutlandırıcı ve **hedef bayt** için
sıkıştırıcı. Terminal aracıdır; grafik arayüzü yoktur, çevrimdışı çalışır ve
tek dosya olarak dağıtılır.

Temel fikir: kullanıcı "PNG olarak kaydet, kalite 80" demez. Kullanıcı hedefi
söyler — *"azami 12 000 bayt, en fazla 1600 piksel genişlik"* — araç kaliteyi,
çözünürlüğü ve renk derinliğini o hedefe ulaşana kadar kendisi arar.

---

## Özellikler

- **PNG okuma (kendi kodumuz)** — imza, `IHDR` (genişlik/yükseklik/bitsel derinlik/
  renk tipi/sıkıştırma/filtre yöntemi), `PLTE`, `tRNS`, `tEXt`/`zTXt`/`iTXt`,
  `eXIf`, `IDAT` zinciri ve `IEND` çerçevelemesi. CRC-32 **her blok için**
  doğrulanır.
- **Beş filtre türü** — `None` (0), `Sub` (1), `Up` (2), `Average` (3), `Paeth` (4),
  yazarken "en küçük mutlak-toplam sapma" sezgiseliyle seçilir, okurken geri alınır.
- **Interlaced (Adam7) PNG açıkça reddedilir** — `interlace_method != 0` için
  ayrı bir hata türü ve ayrı bir hata mesajı vardır (bkz. *Bilinen Sınırlamalar*).
- **PNG yazma (kendi kodumuz)** — filtreli, zlib sıkıştırılmış, 64 KiB'lık
  `IDAT` parçalarına bölünmüş çıktı.
- **DEFLATE için `flate2` (`miniz_oxide`)** — RFC 1950 zlib / RFC 1951 DEFLATE.
  Kendi sıkıştırıcımız **yoktur**; gerekçesi aşağıdadır.
- **Serit tamponlu akış** — görüntünün tamamı belleğe alınmaz. Okuma iki satır
  tamponu + `IDAT` parça tamponu, yeniden boyutlandırma kaynak ve hedef için
  `SatirHalkasi` ile çalışır.
- **Dört yeniden boyutlandırma filtresi** — `kutu` (alan ortalaması),
  `en-yakin`, `bilinear`, `bicubic` (Catmull-Rom).
- **Kırpma, kenar boşluğu, en-boy oranlı hedef** — `--kirpma x,y,g`,
  `--kenar-boslugu`, `--en-fazla-genislik`, `--en-fazla-yukseklik`.
- **Hedef boyutta ikili arama** — `optimize --hedef-boyut`. Kalite tek başına
  yetmezse çözünürlük küçültme adımına geçer ve bunu uyarıyla bildirir.
- **JPEG başlık okuyucu (baseline)** — `SOI`/`APPn`/`DQT`/`SOF0`/`DHT`/`SOS`,
  kuantizasyon tabloları, örnekleme faktörleri, EXIF alan listesi.
  **JPEG yeniden kodlanmaz** (bkz. *Bilinen Sınırlamalar*).
- **Toplu iş kuyruğu** — özyinelemeli `read_dir` gezintisi, uzantı filtresi
  (`png`, `jpg`, `jpeg`), gizli dosya filtresi, derinlik sınırı, çıktı dizinini
  girdi saymama. Tek dosyanın hatası kuyruğu **durdurmaz**.
- **`--dry-run`** — hiçbir dosya yazmadan planı hesaplar ve raporlar.
- **`clap` alt komutları** — `info`, `convert`, `resize`, `optimize`, `batch`.
- **`serde` + `serde_json` rapor** — `--rapor <dosya>` ve `--json`.
- **`#![forbid(unsafe_code)]`**, `#![deny(missing_docs)]`, elle yazılmış
  `Display`/`Error` impl (bu bağımlılıkla `thiserror` kullanılmaz).

### Neden `flate2` ve kendi DEFLATE sıkıştırıcım yok?

Bağımlılık politikası (WORKER_CONTRACT § 3.2-D) kriptografi, sıkıştırma ve karma
algoritmalarını **el yazmanın hatalı ve güvensiz** olduğunu söyler: test vektörüne
karşı sınanmamış bir sıkıştırıcı, kanonik bakımlı bir kütüphaneden **daha**
risklidir. Aynı sözleşme § 3.2-D, bu projeye (`04 PixelMill`) `flate2`'yi açıkça
izin verir ve backend olarak **yalnızca** `miniz_oxide` (saf Rust) kullanılmasını
şart koşar. Backend `miniz_oxide` olduğu için C kütüphanesi bağlanmaz; hedef
makinede `zlib.dll` gerekmez.

Bu karar **kendi yazma isteğiyle çelişmez**: PNG'nin blok/filtre/okuma/yazma
katmanları, CRC-32, Paeth sezgisi ve tüm yeniden boyutlandırma matematiği
tamamen kendi kodumuzdur. `flate2` yalnızca tek bir alt katmanda, tek bir
algoritma için (zlib akışı) devreye girer. RFC 1950/1951 uyumumuz ayrıca
`meta::tests::zlib_acma_calisir` ve tüm PNG gidiş-dönüş testleriyle doğrulanır.

---

## Kurulum

Gereksinim: Rust **1.74** veya üzeri (MSRV). Geliştirme ortamında `cargo 1.98.1` /
`rustc 1.98.1` ile derlendi ve test edildi. Harici C kütüphanesi veya sistem
bağımlılığı yoktur.

```console
$ cargo build --release
   Compiling pixelmill v0.1.0 (%USERPROFILE%\Desktop\Projeler\projects\04-pixelmill)
    Finished `release` profile [optimized] target(s) in 9.29s
```

Üretilen tek dosya: `target\release\pixelmill.exe` — **1 236 371 bayt**.

```console
$ cargo install --path .
  Installing pixelmill v0.1.0 (%USERPROFILE%\Desktop\Projeler\projects\04-pixelmill)
    Finished `release` profile [optimized] target(s) in 1.91s
  Installing %USERPROFILE%\.cargo\bin\pixelmill.exe
  Installed package `pixelmill v0.1.0 (%USERPROFILE%\Desktop\Projeler\projects\04-pixelmill)` (executable `pixelmill.exe`)
```

Doğrulama:

```console
$ pixelmill --version
pixelmill 0.1.0
```

---

## Kullanım

Aşağıdaki **her komut gerçekten çalıştırılmıştır**; çıktılar kopyadır. Örnek
dosyalar: `foto.png` (320×240 RGBA gradyan), `desen.png` (200×150 gri gürültü),
`renk.jpg` (160×120 JPEG).

### `info` — biçim ve gömülü veri

```console
$ pixelmill info foto.png
dosya      : foto.png
bicim      : PNG
boyut      : 320x240
renk tipi  : gercek renk + alfa (RGBA) (6)
bit derin. : 8
palet      : 0 giris
sekme      : 0
bloklar    : IHDR, sRGB, gAMA, pHYs, IDAT, IDAT, IEND
exif       : yok
```

JPEG'de yalnızca başlık ve kuantizasyon tabloları okunur:

```console
$ pixelmill info renk.jpg
dosya      : renk.jpg
bicim      : JPEG
boyut      : 160x120
bilesen    : 3 (en yuksek ornek carpimi 4)
  bilesen 1 : 2x2 kuantizasyon tablosu 0
  bilesen 2 : 1x1 kuantizasyon tablosu 1
  bilesen 3 : 1x1 kuantizasyon tablosu 1
kuantizasyon tablolari: 2
  tablo 0 : 0 bit, DC carpani 8
  tablo 1 : 0 bit, DC carpani 9
huffman tablolari    : 5
APP segmentleri      : [0]
JFIF surumu          : Some(1)
exif                 : yok
markerlar            : APP0, DQT, DQT, SOF0, DHT, DHT, DHT, DHT, SOS
```

### `resize` — yeniden boyutlandırma

```console
$ pixelmill resize foto.png kucuk.png --genislik 160 --yukseklik 120 --filtre bicubic
PixelMill 0.1.0 | kuru calistirma: hayir | toplam: 1 basarili: 1 basarisiz: 0
OK   foto.png -> kucuk.png (173 bayt, kalite 80)
```

En-boy oranını koruyarak küçültme (dördüncü boyut otomatik hesaplanır):

```console
$ pixelmill resize desen.png dar.png --en-fazla-genislik 100 --filtre kutu
PixelMill 0.1.0 | kuru calistirma: hayir | toplam: 1 basarili: 1 basarisiz: 0
OK   desen.png -> dar.png (1929 bayt, kalite 80)
```

Yalnızca **genişlik** sınırı verilmiştir; yükseklik en-boy oranından türetilir
(200×150 → 100×75). Görüntü hiçbir zaman büyütülmez.

### `optimize` — hedef bayta ikili arama

Kalite tek başına yetmezse araç çözünürlüğü küçültür ve bunu **açıkça uyarır**:

```console
$ pixelmill optimize desen.png desen_kucuk.png --hedef-boyut 8000
PixelMill 0.1.0 | kuru calistirma: hayir | toplam: 1 basarili: 1 basarisiz: 0
OK   desen.png -> desen_kucuk.png (7351 bayt, kalite 1)
     uyari: hedefe ulasmak icin cozunurluk 131x98 degerine kucultuldu
```

Hedef zaten kalite 100'de aşılıyorsa hiçbir adım gerekmez:

```console
$ pixelmill optimize foto.png hedef.png --hedef-boyut 12000
PixelMill 0.1.0 | kuru calistirma: hayir | toplam: 1 basarili: 1 basarisiz: 0
OK   foto.png -> hedef.png (812 bayt, kalite 100)
```

### `convert` — sabit kalite ile yeniden kodlama

```console
$ pixelmill convert foto.png donusturulmus.png --kalite 60
PixelMill 0.1.0 | kuru calistirma: hayir | toplam: 1 basarili: 1 basarisiz: 0
OK   foto.png -> donusturulmus.png (787 bayt, kalite 60)
```

### `batch` — toplu iş, kuru çalıştırma ve hata ayrımı

```console
$ pixelmill batch . --cikti-dizini ./cikti --en-fazla-genislik 160 --kuru-calistir
PixelMill 0.1.0 | kuru calistirma: evet | toplam: 3 basarili: 2 basarisiz: 1
OK   desen.png -> cikti/desen.png (4207 bayt, kalite 80)
OK   foto.png -> cikti/foto.png (173 bayt, kalite 80)
HATA renk.jpg (kodlama) - JPEG piksel isleme kapsam disi (yeniden kodlama): PixelMill JPEG'de
yalnizca baslik ve kuantizasyon tablosunu okur, yeniden kodlama yapmaz
     uyari: JPEG desteklenir ancak yeniden kodlama kapsam disidir; dosya atlandi
```

Tek bir hatalı dosya kuyruğu durdurmaz, kalan iki dosya işlenir. `--kuru-calistir`
hiçbir çıktı yazmaz; aynı komut `--kuru-calistir` olmadan çalıştırıldığında
`cikti/` klasörüne iki dosya yazar. JPEG girdisi atlanır ve `kodlama` hata
sınıfıyla rapora yazılır.

---

## Test

```console
$ cargo test
running 124 tests
...
test result: ok. 124 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.15s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 16 tests
...
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Test sonucu: okunan 140; başarısız 0** (124 birim + 16 entegrasyon).

Kapsanan kenar durumları:

| Alan | Testler |
|---|---|
| PNG gidiş-dönüş | `png_kodla_gidis_donus_basar`, `cok_satirli_govde_idat_parcalarina_bolunur` (çok parçalı `IDAT`), `rgba_gidis_donus_basar`, `gri_png_gidis_donus_basar`, `rgb_png_alfa_kaybolur_ve_geri_donus_dogru`, `on_alti_bit_yuksek_bayta_indirgenir` |
| Bozuk CRC | `bozuk_crc_hata_dogrular` (blok), `bozuk_crc_hata_dondurur` (dosya), `crc_bir_bit_degisiminde_degisir` |
| Bozuk IHDR | `ihdr_kisa_olunca_hata_dondurur`, `sifir_boyutlu_baslik_reddedilir`, `sikistirma_ve_filtre_yontemi_sifir_olmali`, `gecersiz_kombinasyonlar_reddedilir` |
| Interlaced reddi | `sekmeli_png_acikca_reddedilir`, `interlacing_reddi_mesaji_spesifik` |
| Palette PNG | `paletli_secenek_derinligi_paletten_turetir`, `paletsiz_secenek_palet_tipi_reddeder`, `paletli_secenek_giris_sinirlarini_kontrol_eder`, `trns_ayirt_edici_dogru` |
| Grayscale PNG | `gri_png_gidis_donus_basar`, `gri_tespiti_renk_esitligine_bakar` |
| Alfa kanalı | `alfa_ata_tum_piksellere_uygulanir`, `sifir_govde_tam_saydam`, `buyutme_ve_alfa_korunumu` |
| 1×1 görüntü | `tek_piksel_govde_calisir` |
| Sıfır boyut | `sifir_boyutlu_baslik_reddedilir`, `sifir_boyut_reddedilir` (görsel + hedef), `sifir_govde_tam_saydam` |
| Aşırı boyut reddi | `asiri_boyut_reddedilir`, `asiri_bayt_indirgeme_yapar`, `asiri_uzunluk_sinir_reddi_verir`, `asiri_hedef_boyut_reddedilir` |
| Filtre türleri | `bes_filtre_tipi_gerialma_gidis_donus_yapar`, `sub_filtresi_soldaki_degeri_cikarir`, `up_filtresi_ust_satiri_cikarir`, `average_filtresi_ortalamayi_cikarir`, `paeth_komsu_secimi_temel_kurallara_uyar`, `paeth_tam_zaman_girdileri_guvenli`, `filtre_sifir_ham_veriyi_kopyalar`, `gecersiz_filtre_tipi_hata_dondurur`, `kisa_onceki_satir_hata_dondurur` |
| Filtre sezgisi | `en_iyi_filtre_duz_satirda_none_secer`, `en_iyi_filtre_dikey_desende_up_secer`, `en_iyi_filtre_her_zaman_gecerli_tip_dondurur` |
| Yeniden boyutlama sapması | `kutu_kucultmede_cok_nokta_toplar`, `kutu_kismi_kapsama_agirligini_hesaplar`, `bilineer_iki_nokta_kullanir`, `bicubic_dort_nokta_kullanir`, `agirliklar_her_zaman_birim_toplamli`, `bicubic_tam_noktada_birim_dik`, `bicubic_yarim_noktada_simetrik`, `ayni_girdi_ayni_sonuc_verir`, `yetersiz_kaynak_satir_hata_dondurur` |
| Tek eksenli sınır | `en_fazla_genislik_tek_basina_kucultur` (200×150 → 100×75), `en_fazla_genislik_gorseli_buyutmez` (40×30 + sınır 200 → 40×30) |
| Hedef boyut araması | `hedef_boyut_arama_butceye_uyar` (entegrasyon), `hedefe_sikistirma_boyutu_kucultur` |
| Kırpma | `kirpma_istenen_penceresi_verir` (entegrasyon) |
| Kuru çalıştırma | `dry_run_hicbir_dosya_yazmaz`, `cli_batch_kuru_calistirma_dosya_yazmaz` |
| Toplu kuyruk | `toplu_kuyruk_bir_hata_digerini_durdurmaz`, `gizliler_dahil_secenegi_dosyayi_alir` |
| Bozuk dosya atlanıyor | `toplu_kuyruk_bir_hata_digerini_durdurmaz`, `yarim_kalmis_dosya_hata_dondurur`, `cli_bozuk_dosyada_hata_kodu_dondurur` |
| Gizli dosya | `gizliler_dahil_secenegi_dosyayi_alir` |
| Uzantı filtresi | `toplu_kuyruk_bir_hata_digerini_durdurmaz` (`.txt` kuyruğa girmez) |
| Üzerine yazmama | `cikti_kaynaga_ustune_yazmaz` |
| Atomik yazım | `gecici_yol` + `atomik_yaz`; yarım dosya hedef konumda görünmez |
| Meta veri | `tEXt`/`zTXt` (zlib ile)/`eXIf` ayrıştırma, `exif_konum_isaretcisi_bulunur`, `anahtar_dogrulama_kurallari`, bozuk EXIF bayt sırası reddi |
| JPEG başlık | `dqt_ayir`, `jpeg_baslik_oku`, `marker_adi`, `piksel_kodlama_desteklenmiyor` |
| zlib uyumu | `zlib_acma_calisir`, `ztxt_blogu_zlib_ile_cozulur` (RFC 1950) |
| Sınırlar | `sinirlar_birer_kiyim_deger`, `bicubic_agirliklari_birim_toplamli` |

Testler deterministiktir: rastgelelik crate'i yoktur (sabit tohumlu LCG), ağ
erişimi yoktur, geçici dosyalar `std::env::temp_dir()` altında
`std::process::id()` ile benzersizleştirilir ve `Drop` ile temizlenir. `Drop`
temizliği `let _ =` ile bilinçli olarak yutulur, çünkü `Drop` içinden hata
döndürülemez; bu, sözleşmenin sessiz yutma yasağına `Drop` temizliği istisnasıdır.

Diğer kapılar:

```console
$ cargo build --release
    Finished `release` profile [optimized] target(s) in 9.29s

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [optimized] target(s) in 1.47s

$ cargo fmt --check
$ echo $?
0
```

---

## Proje Yapısı

```
04-pixelmill/
├── Cargo.toml
├── Cargo.lock              (üretilir, commit edilir)
├── LICENSE.txt             MIT, tam metin
├── README.md
├── .gitignore
├── src/
│   ├── main.rs             (220)  CLI kabuğu: alt komut yönlendirme, çıktı
│   ├── lib.rs              ( 75)  modül ağacı, `forbid(unsafe_code)`, yeniden ihrac
│   ├── hata.rs             (355)  `Hata` enum'u + elle `Display`/`Error`
│   ├── sinir.rs            (122)  güvenlik üst sınırları (bellek bombası koruması)
│   ├── gorsel.rs           (392)  RGBA `Raster`, satır okuyucu soyutlaması
│   ├── png/
│   │   ├── mod.rs          ( 18)
│   │   ├── blok.rs         (287)  blok çerçevelemesi, CRC-32, uzunluk sınırları
│   │   ├── filtre.rs       (374)  filtre uygulama/geri alma, en iyi filtre sezgisi
│   │   ├── okuma.rs        (1130) imza/IHDR/IDAT okuma, örnekleme genişletme
│   │   └── yazma.rs        (615)  filtreleme, zlib, `IDAT` parçalama
│   ├── jpeg.rs             (454)  JPEG başlık + kuantizasyon tablosu okuyucu
│   ├── meta.rs             (611)  tEXt/zTXt/iTXt/eXIf okuma, anahtar doğrulama
│   ├── boyut.rs            (587)  kutu/en-yakin/bilinear/bicubic + `SatirHalkasi`
│   ├── kip.rs              (200)  kırpma, kenar boşluğu, en-boy oranlı hedef
│   ├── kodlama.rs          (356)  kalite → palet kademesi, piksel formu
│   ├── hedef.rs            (279)  hedef bayt için ikili arama
│   ├── kuyruk.rs           (178)  özyinelemeli gezinti, uzantı/gizli filtreleri
│   ├── islem.rs            (431)  tek dosya/ klasör iş hattı, atomik yazım
│   ├── rapor.rs            (238)  `serde` rapor yapıları + JSON
│   └── cli.rs              (341)  `clap` tanımları
└── tests/
    └── entegrasyon.rs      (374)  16 uçtan uca test + `GeciciDizin` yardımcısı
```

**Toplam: 7 696 satır** (Rust + yapılandırma: 7 637 `.rs` + `Cargo.toml` 33 +
`LICENSE.txt` 17 + `.gitignore` 9). Modüller arası bağımlılık yönü tek yönlüdür:
`png`/`jpeg`/`meta` → `gorsel` → `boyut`/`kip` → `kodlama` → `hedef` → `islem` →
`rapor` → `cli`.

---

## Yapılandırma

Ayar dosyası yoktur; tüm yapılandırma komut satırı bayraklarından gelir. Aşağıdaki
tablolar `pixelmill <alt komut> --help` çıktısından ve kaynak koddan birebir
alınmıştır.

### Ortak dönüşüm seçenekleri (`convert`, `optimize`)

| Bayrak | Varsayılan | Etkisi |
|---|---|---|
| `--kalite <0-100>` | `80` | Sabit kalite. `90-100` = tam renk (kayıpsız), `70-89` = 8 renkli palet, `45-69` = 64 renkli palet, `<45` = 256 renkli palet. |
| `--hedef-boyut <BAYT>` | yok | Verilirse ikili arama yapılır; `--kalite` yok sayılır. `0` reddedilir. |
| `--en-fazla-genislik <PIKSEL>` | yok | En-boy oranını koruyarak küçültür. |
| `--en-fazla-yukseklik <PIKSEL>` | yok | En-boy oranını koruyarak küçültür. |
| `--filtre <FILTRE>` | `bicubic` | `kutu`, `en-yakin`, `bilinear`, `bicubic`. |
| `--metni-koru` | kapalı | `tEXt` alanlarını koru (varsayılan: sil). |
| `--konumu-koru` | kapalı | `eXIf` / EXIF konum verisini koru (varsayılan: sil). |
| `--rapor <DOSYA>` | yok | JSON raporunu dosyaya yazar. |
| `--json` | kapalı | Raporu standart çıktıya basar. |
| `--kuru-calistir` | kapalı | Planı hesaplar, hiçbir dosya yazmaz. |

### `resize` ek seçenekleri

| Bayrak | Varsayılan | Etkisi |
|---|---|---|
| `--genislik <PIKSEL>` | yok | Hedef genişlik. `--yukseklik` ile **birlikte** verilmelidir. |
| `--yukseklik <PIKSEL>` | yok | Hedef yükseklik. `--genislik` ile birlikte verilmelidir. |
| `--kirpma <X,Y,G>` | yok | Kırpma penceresi; dördüncü değer otomatik hesaplanır. `G=0` reddedilir. |
| `--kenar-boslugu <PIKSEL>` | `0` | Çerçeveye eklenen kenar boşluğu. |

### `batch` ek seçenekleri

| Bayrak | Varsayılan | Etkisi |
|---|---|---|
| `--cikti-dizini <DIZIN>` | zorunlu | Kaynak ağacının içindeyse gezintide atlanır. |
| `--gizlileri-dahil` | kapalı | `.` ile başlayan ve Windows `FILE_ATTRIBUTE_HIDDEN` taşıyan dosyaları dahil eder. |
| `--hedef-boyut`, `--kalite`, `--en-fazla-genislik`, `--en-fazla-yukseklik`, `--kuru-calistir`, `--rapor`, `--json` | — | Yukarıdaki ortak seçeneklerle aynı. |

### Kalite → palet kademesi eşlemesi

`kodlama::Kademe::kaliteden` kaliteyi dört kademeye eşler. Bu eşleme PNG'ye
gömülen bir kalite sayısı değil, **renk derinliği** seçimidir; bu yüzden
`--kalite 60` çıktısı 8-bit değil, 2-bit kanal başına paletli PNG olabilir.
Piksel bazında kayıpsız gidiş-dönüş için `--kalite 100` gerekir.

### Güvenlik üst sınırları (`src/sinir.rs`)

| Sabit | Değer | Amaç |
|---|---|---|
| `EN_FAZLA_BLOK` | 32 MiB (33 554 432) | Tek bloğun veri uzunluğu üst sınırı. |
| `EN_FAZLA_META_SEGMENT` | 1 MiB (1 048 576) | `IDAT` dışı blokların toplamı. |
| `EN_FAZLA_PIKSEL` | 200 000 000 | Görüntünün piksel sayısı üst sınırı. |
| `EN_FAZLA_KENAR` | 100 000 | Genişlik/yükseklik üst sınırı. |
| `EN_FAZLA_DERINLIK` | 32 | Gezinti derinliği (simgelerin döngüye girmesine karşı). |
| `BICUBIC_YARICAP` | 2 | Bicubic filtrenin destek yarıçapı (4 komşu nokta). |

Ayrıca `gorsel::piksel_sayisi`, `width * height * 4` çarpımı `usize`'a
sığmıyorsa `0` döndürerek taşmayı sessizce üretmez; `Raster::yeni_sifir` bunu
açık bir boyut hatasına çevirir (`asiri_boyut_reddedilir` testi).

---

## Bilinen Sınırlamalar

Bunlar **bilinçli** kararlardır; gizlenmemiştir.

1. **Interlaced (Adam7) PNG reddedilir.** `interlace_method != 0` olan dosyalar
   `Hata::PngSekmeliDesteklenmiyor` ile açıkça reddedilir; sessizce bozuk çözülmez.
   Gerekçe: Adam7 yedi geçişli tarama düzeni serit tamponlu iş hattımızla
   doğrudan uyuşmuyor ve serit akışını bozuyordu. `sekmeli_png_acikca_reddedilir`
   testi bu davranışı sabitler.
2. **JPEG yeniden kodlanmaz.** Yalnızca başlık, kuantizasyon tabloları, Huffman
   tabloları, örnekleme faktörleri ve EXIF alanları okunur. `convert`/`resize`/
   `optimize` JPEG girdisinde dosyayı atlar ve raporda `kodlama` hata sınıfıyla
   listeler. Gerekçe: tam JPEG DCT kodlayıcısı MVP kapsamı dışındadır ve
   libjpeg-turbo gibi bir C kütüphanesi bağımlılığı politikayla yasaktır.
3. **WebP, AVIF, JPEG 2000 desteklenmez.** Konsept raporunda listelenen dört
   gömülü kodlayıcının yerine bu MVP'de yalnızca PNG vardır.
4. **PNG "kalitesi" bir kuantizasyon kademesidir**, klasik JPEG kalite eğrisi
   değildir. Algılanan kalite ile dosya boyutu arasındaki ilişki, PNG'nin
   kayıpsız zlib ile sıkıştırılması nedeniyle JPEG'den farklıdır.
5. **Animasyonlu PNG (`acTL`) reddedilir.** APNG kapsam dışıdır.
6. **Eşzamanlılık yok.** Konsept raporunda önerilen "dosya başına iş parçacığı,
   en fazla 4 eşzamanlı dosya" uygulanmamıştır; işlem sıralıdır. Gerekçe:
   `forbid(unsafe_code)` ve öngörülebilir bellek davranışı birincil amaçtır ve
   MVP'de iş parçacığı kazancı ölçülmemiştir.
7. **Kırpmada `--genislik`/`--yukseklik` çifti zorunludur.** Yalnızca biri
   verilirse ayar geçersiz sayılır (oransal küçültme için `--en-fazla-*`
   bayrakları kullanılmalıdır).
8. **16-bit örnekler 8-bit'e indirgenir** (PNG spec'inin gerektirdiği gibi
   yüksek bayt kullanılır). 16-bit çıktı üretilmez.
9. **Görüntü çözümü diskte tam tutulmaz, hedef gövde tutulur.** Kaynak satırları
   akıştan gelir; hedef gövde bellekte olmak zorundadır. Çok büyük hedeflerde
   bu, `width * height * 4` bayt demektir.
10. **Konsept raporundaki bellek/arayüz ölçütleri ölçülmemiştir.** Sürükle-bırak
    arayüz, karşılaştırma yüzeyi, filigran ve oturum kalıcılığı bu MVP'de yoktur
    (grafik arayüz crate'i bağımlılık politikasıyla yasaktır).
11. `Drop` içindeki geçici dizin temizliği hataları `let _ =` ile yutulur;
    `Drop`'tan hata döndürülemez. Bu, sözleşmedeki "sessiz yutma" yasağının
    açıkça belgelenmiş `Drop` temizliği istisnasıdır.
12. `#[allow]` niteliği **hiçbir yerde kullanılmamıştır**; `clippy` temizdir.

---

## Gelecek Geliştirmeler

- Serit akışını bozmadan Adam7 (interlaced) PNG okuma desteği.
- Baseline JPEG **kodlayıcı** (kendi DCT kuantizasyon yazımı; C kütüphanesi
  olmadan) ve `optimize` aramasının JPEG'e uygulanması.
- Çok iş parçacıklı, bellek tavanı denetimli toplu iş (kuyruk zaten sıralı ve
  tekrarlanabilir; iş birimleri değişmeden paralel çalıştırılabilir).
- Kırpma aracı ve filigran (konsept v2 aşaması).
- Ölçüm: fikir raporundaki bellek bütçesi (≤ 260 MB tepe RSS) henüz ölçülmedi;
  ölçüm matrisi (`biçim × hız derecesi`) eklenmeli.

---

## Troubleshooting

### 1) `PNG CRC hatasi (sRGB): dosyada 0xaece1ce9, hesaplanan 0xd9c92c7f`

**Belirti:** Dosya PNG imzalı ve `IHDR` geçerli, ama işlem başarısız.

**Neden:** Bir bloğun verisi değişmiş, CRC'si değişmemiştir — dosya taşıma
sırasında bozulmuş ya da kasten düzenlenmiştir. Bu **kasıtlı** bir güvenlik
davranışıdır: bozuk `IDAT` ile sessizce yanlış pikseller üretmektense durulur.

**Çözüm:** Dosyayı yeniden indirin/kopyalayın. Toplu işte bu dosya atlanır ve
raporda `okuma` hata sınıfıyla listelenir; kalan dosyalar işlenmeye devam eder.

### 2) `cikti yolu kaynak dosyayla ayni, ustune yazma yapilmaz: <yol>`

**Belirti:** `convert`/`resize`/`optimize` hiçbir şey yapmadan hata verir.

**Neden:** Konsept raporundaki kabul kriteri: "araç hiçbir koşulda mevcut
dosyanın üzerine yazmaz". Girdi ve çıktı yolunun aynı olması bu kuralın en
yaygın ihlali olurdu.

**Çözüm:** Farklı bir çıktı yolu verin veya `batch --cikti-dizini` kullanın.

### 3) `bilinmeyen bicim: <yol>; yalnizca PNG (.png) ve JPEG (.jpg/.jpeg) taninir`

**Belirti:** Dosya adı `.png` olmasına rağmen "bilinmeyen biçim" hatası.

**Neden:** Biçim tespiti **uzantıya değil, dosyanın imza baytlarına** bakar.
`Set-Content` ile yazılmış metin dosyası, adı `.png` olsa bile PNG imzası
taşımaz.

**Çözüm:** Dosyanın gerçekten PNG olduğundan emin olun. Doğrulamak için
`pixelmill info <yol>` çalıştırın; geçerli bir PNG'de `bloklar` satırı
`IHDR, ..., IEND` gösterir.

### 4) `error: invalid value 'box' for '--filtre <FILTRE>'`

**Belirti:** `clap` hata kodu 2 ile durur.

**Neden:** `--filtre` bir `ValueEnum`; İngilizce karşılıklar (`box`, `nearest`)
kabul edilmez. Geçerli değerler: `kutu`, `en-yakin`, `bilinear`, `bicubic`.

**Çözüm:** `--filtre kutu` kullanın. Tam liste için `pixelmill resize --help`.

### 5) `uyari: hedefe ulasmak icin cozunurluk 131x98 degerine kucultuldu`

**Belirti:** `optimize` hedefe ulaştı ama kalite 1'e düştü ve çözünürlük de
küçüldü.

**Neden:** Hedef bayt, kaliteyi sıfıra indirmekle bile mümkün değildi. Konsept
raporundaki sıra uygulanır: renk derinliği → çözünürlük → kalite (en son çare).

**Çözüm:** Hedefi gerçekçi belirleyin veya `--en-fazla-genislik` vererek
kabul edilebilir bir çözünürlük sınırı koyun. Uyarıyı yok saymayın: kalite 1
çıktı görsel olarak kullanılamaz.

### 6) `sekmeli (Adam7) PNG desteklenmiyor: interlace_method=1; ...`

**Belirti:** Bazı PNG dosyaları açılıyor, bazıları açılmıyor.

**Neden:** Interlaced (Adam7) PNG desteklenmiyor ve bu bilinçli bir karardır
(*Bilinen Sınırlamalar* 1). Araç hatayı sessizce yutmaz.

Gerçek çıktı:

```console
$ pixelmill convert sekmeli.png y.png
pixelmill: sekmeli (Adam7) PNG desteklenmiyor: interlace_method=1; PixelMill yalnizca
interlace_method=0 okur, satirlari tek geciste isler
```

**Çözüm:** Görüntüyü interlaced olmayan bir PNG olarak yeniden kaydedin
(`--kuru-calistir` ile test edin) veya hedefi büyütün.

---

## Atıflar

### Spesifikasyonlar

- **PNG (Portable Network Graphics) Specification, Version 1.2** — W3C,
  <https://www.w3.org/TR/PNG/> (blok çerçevelemesi, filtreler, `IHDR`/`PLTE`/
  `tRNS`/`tEXt`/`zTXt`/`iTXt`/`eXIf` blokları)
- **RFC 2083 — PNG (Portable Network Graphics) Specification Version 1.2**,
  <https://www.rfc-editor.org/rfc/rfc2083>
- **RFC 1950 — Zlib Compressed Data Format Specification version 1.2**,
  <https://www.rfc-editor.org/rfc/rfc1950>
- **RFC 1951 — DEFLATE Compressed Data Format Specification version 1.2**,
  <https://www.rfc-editor.org/rfc/rfc1951>
- **ITU-T T.81 — JPEG (ISO/IEC 10918-1)**, kuantizasyon tabloları, `SOF0`
  baseline bileşenleri, Huffman tabloları
  <https://www.itu.int/rec/T-REC-T.81>
- **C. publicly known CRC-32 (ISO 3309 / ITU-T V.42 polynomial)** — PNG blok
  CRC'si ve `crc32_tip_ile` uygulaması
- **C. Kılıç — Paeth predictör**, PNG spec bölüm 9.2.5, filtre tipi 4

### Rust kütüphaneleri

- `flate2` (DEFLATE/zlib) — <https://docs.rs/flate2/>
- `miniz_oxide` (`flate2` backend'i) — <https://docs.rs/miniz_oxide/>
- `clap` (komut satırı arayüzü) — <https://docs.rs/clap/>
- `serde` (rapor şeması türetme) — <https://serde.rs/>
- `serde_json` (JSON rapor çıktısı) — <https://docs.rs/serde_json/>
- Rust standart kütüphane belgeleri — <https://doc.rust-lang.org/std/>
- `cargo` yerel rehberi — <https://doc.rust-lang.org/cargo/>

### Doğrudan kopyalanan kod

Bu depoda dış projelerden kopyalanan kod parçası **yoktur**. PNG blok/filtre
mantığı, CRC-32 tablosu, Paeth sezgisi, palet kuantizasyonu ve yeniden
boyutlandırma matematiği sıfırdan yazılmış; referans olarak yalnızca
yukarıdaki spesifikasyonlar kullanılmıştır.

### İç tasarım kaynağı

- Rapor dosyasının kendisi: `%USERPROFILE%\Desktop\Fikirler\04-piksel-atolyesi.html`
  (yerel yol; URL değildir). Bölümler `b01`, `b03`, `b05`, `b07`, `b08`, `b09`,
  `b16` bu projenin kapsam ve gerekçelerini belirledi: hedef boyut araması
  stratejisi (`b05`), hedef arama örneği (`b07`), bellek bütçesi (`b08`),
  taşınabilirlik/konum çözümlemesi (`b09`).

### Kalite kapısı ve bağımlılık politikası

- `WORKER_CONTRACT.md` ve `MANIFEST.md` kartı 04 — bağımlılık izin listesi
  (§ 3.2), kalite kuralları (§ 4) ve teslim kontrol listesi (§ 10)

---

## Lisans

MIT — tam metin için [LICENSE.txt](LICENSE.txt) dosyasına bakınız.

Telif: `Copyright (c) 2026 PixelMill contributors`
