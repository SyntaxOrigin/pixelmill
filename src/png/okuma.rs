//! PNG okuyucu: basligi, paleti ve pikselleri **satir akisiyla** cozer.
//!
//! ## Akis
//!
//! 1. Imza (8 bayt) ve `IHDR` dogrulanir.
//! 2. `IDAT` oncesi gelen bloklar (`PLTE`, `tRNS`, `tEXt`, ...) toplanir.
//! 3. `IDAT` yalnizca zlib **cozme girdisi** olarak kullanilir; ham IDAT baytlari
//!    en fazla bir blogun uzunlugu kadar tamponlanir (`EN_FAZLA_BLOK`).
//! 4. Her tarama satiri ayri ayri cozulur, filtresi geri alinir ve RGBA'ya
//!    genisletilerek govdeye yazilir. **Dosyanin tamami hicbir zaman okunmaz.**
//!
//! ## Desteklenenler ve desteklenmeyenler
//!
//! | Ozellik | Durum |
//! |---------|-------|
//! | Renk tipi 0 (gri), 2 (RGB), 3 (palet), 4 (gri+alfa), 6 (RGBA) | desteklenir |
//! | Bit derinligi 1, 2, 4, 8, 16 (16-bit yuksek bayta indirgenir) | desteklenir |
//! | `tRNS` saydamlik anahtari / palet alfalari | desteklenir |
//! | Filtre tipleri 0..=4 | desteklenir |
//! | `interlace_method = 1` (Adam7) | **acikca reddedilir** |
//! | Animasyonlu PNG (`acTL`) | **acikca reddedilir** |
//!
//! Interlacing'in reddi bir "eksiklik" degil, bilincli bir tasarim kararidir:
//! Adam7 yedi gecislik bir serit duzenidir ve "satiri sirayla isle" bellek
//! modelimizi bozardi. Hata mesaji `interlace_method` degerini verir.

use std::cell::Cell;
use std::io::Read;
use std::path::Path;
use std::rc::Rc;

use crate::gorsel::{Raster, KANAL};
use crate::hata::{io_hata, Hata};
use crate::meta::{png_metin_ayikla, MetinKaydi};
use crate::png::blok::{basligi_ayir, crc32_tip_ile, tip_metin, uzunlugu_dogrula, PNG_IMZASI};
use crate::png::filtre::{filtreyi_geri_al, EN_FAZLA_FILTRE_TIPI};

/// PNG `IHDR` basliginin cozulmus hali.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Baslik {
    /// Gorselinin genisligi (piksel).
    pub genislik: u32,
    /// Gorselinin yuksekligi (piksel).
    pub yukseklik: u32,
    /// Ogrenek derinligi (piksel basina kanal basina bit).
    pub bit_derinligi: u8,
    /// Renk tipi kodu (0, 2, 3, 4, 6).
    pub renk_tipi: u8,
    /// Sikistirma yontemi (PNG icin daima 0).
    pub sikistirma: u8,
    /// Filtre yontemi (PNG icin daima 0).
    pub filtre_yontemi: u8,
    /// Sekmelendirme yontemi (0 = yok, 1 = Adam7).
    pub sekme: u8,
}

impl Baslik {
    /// Renk tipine gore kanal sayisini dondurur (PNG spec, tablo 11.1).
    #[must_use]
    pub fn kanal_sayisi(&self) -> Option<u8> {
        match self.renk_tipi {
            0 | 3 => Some(1),
            2 => Some(3),
            4 => Some(2),
            6 => Some(4),
            _ => None,
        }
    }

    /// Filtrelenmemis bir tarama satirindaki bayt sayisini dondurur.
    ///
    /// `(bit_derinligi * kanal * genislik)` toplam bit sayisinin yukari
    /// yuvarlanmis 8'le bolunmus halidir; alt-bayt derinlikleri bu sayede
    /// "satur basina bir bayt tamamlanana kadar" dogru sekilde hesaplanir.
    #[must_use]
    pub fn satir_bayt(&self) -> usize {
        let kanal = self.kanal_sayisi().unwrap_or(1);
        let bit = u64::from(self.bit_derinligi) * u64::from(kanal) * u64::from(self.genislik);
        bit.div_ceil(8) as usize
    }

    /// Filtrelemede kullanilan "piksel basina bayt" (`bpp`, en az 1).
    #[must_use]
    pub fn piksel_bayt(&self) -> usize {
        let kanal = u32::from(self.kanal_sayisi().unwrap_or(1));
        ((u32::from(self.bit_derinligi) * kanal) / 8).max(1) as usize
    }

    /// Renk tipinin insan tarafindan okunabilir adi.
    #[must_use]
    pub fn renk_tipi_adi(&self) -> &'static str {
        match self.renk_tipi {
            0 => "gri (grayscale)",
            2 => "gercek renk (truecolor)",
            3 => "palet (indexed)",
            4 => "gri + alfa",
            6 => "gercek renk + alfa (RGBA)",
            _ => "gecersiz",
        }
    }
}

/// `IHDR` alanlarini dogrular ve hatalari `Result` ile dondurur.
///
/// # Hatalar
///
/// Renk tipi/bit derinligi kombinasyonu tanimsizsa, sikistirma veya filtre
/// yontemi 0 degilse, interlaced ise veya boyut sinirlari asiliyorsa hata doner.
pub fn basligi_dogrula(baslik: &Baslik) -> Result<(), Hata> {
    if baslik.sikistirma != 0 {
        return Err(Hata::PngBaslikBozuk {
            ayrinti: format!("sikistirma yontemi 0 olmali, verilen {}", baslik.sikistirma),
        });
    }
    if baslik.filtre_yontemi != 0 {
        return Err(Hata::PngBaslikBozuk {
            ayrinti: format!("filtre yontemi 0 olmali, verilen {}", baslik.filtre_yontemi),
        });
    }
    // Interlace kontrolu boyut kontrolunden **once** yapilir: kullaniciya
    // "dosyan cok buyuk" degil, "bu bicim desteklenmiyor" demeliyiz.
    if baslik.sekme != 0 {
        return Err(Hata::PngSekmeliDesteklenmiyor {
            yontem: baslik.sekme,
        });
    }
    let kanal = match baslik.kanal_sayisi() {
        Some(k) => k,
        None => {
            return Err(Hata::PngRenkTipiGecersiz {
                renk_tipi: baslik.renk_tipi,
                bit_derinligi: baslik.bit_derinligi,
            })
        }
    };
    let gecerli = match baslik.renk_tipi {
        0 => matches!(baslik.bit_derinligi, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => kanal > 1 && matches!(baslik.bit_derinligi, 8 | 16),
        3 => matches!(baslik.bit_derinligi, 1 | 2 | 4 | 8),
        _ => false,
    };
    if !gecerli {
        return Err(Hata::PngRenkTipiGecersiz {
            renk_tipi: baslik.renk_tipi,
            bit_derinligi: baslik.bit_derinligi,
        });
    }
    crate::gorsel::boyut_dogrula(baslik.genislik, u64::from(baslik.yukseklik))
}

/// Bir `IHDR` veri blogunu ayristirir (tam 13 bayt beklenir).
///
/// # Hatalar
///
/// Uzunluk 13 degilse veya icerik gecersizse hata doner.
pub fn ihdr_ayir(veri: &[u8]) -> Result<Baslik, Hata> {
    if veri.len() != 13 {
        return Err(Hata::PngBaslikBozuk {
            ayrinti: format!("IHDR 13 bayt olmali, verilen {}", veri.len()),
        });
    }
    let baslik = Baslik {
        genislik: u32::from_be_bytes([veri[0], veri[1], veri[2], veri[3]]),
        yukseklik: u32::from_be_bytes([veri[4], veri[5], veri[6], veri[7]]),
        bit_derinligi: veri[8],
        renk_tipi: veri[9],
        sikistirma: veri[10],
        filtre_yontemi: veri[11],
        sekme: veri[12],
    };
    basligi_dogrula(&baslik)?;
    Ok(baslik)
}

/// `IDAT` bloklarini yalnizca veri akisi olarak sunan, CRC dogrulayan okuyucu.
///
/// Bu tip `std::io::Read` uygulamaz; inflate surucusu (`flate2::Decompress`)
/// tarafindan dogrudan beslenir. Boylece her bayt icin `io::Error` donusum
/// zinciri kurulmaz ve hata sinifimiz (`Hata`) korunur.
pub struct IdatOkisi<R: Read> {
    kaynak: R,
    izgara: Vec<u8>,
    tampon: Vec<u8>,
    konum: usize,
    bitti: bool,
    /// `IDAT` disi bloklarin `uzunluk + tip + veri` baytlari (sinirli).
    meta: Vec<u8>,
    /// Gorulen blok tipleri (ayiklama ve `info` raporu icin).
    pub bloklar: Vec<String>,
    /// `PLTE` blogundan gelen palet (en fazla 256 giris).
    pub palet: Vec<[u8; 3]>,
    /// `tRNS` blogunun ham icerigi, varsa.
    pub trns: Option<Vec<u8>>,
    /// Ilk `IDAT` blogu oncesi okunmus ve dogrulanmis baslik.
    pub baslik: Baslik,
}

impl<R: Read> IdatOkisi<R> {
    /// Imza + `IHDR` + `IDAT` oncesi bloklari okuyarak akisi baslatir.
    ///
    /// # Hatalar
    ///
    /// Imza, `IHDR`, `PLTE` veya CRC hatasi varsa hata doner. `acTL` gorulurse
    /// animasyonlu PNG reddedilir.
    pub fn yeni(mut kaynak: R) -> Result<Self, Hata> {
        let mut izgara = Vec::new();
        izgara_doldur(&mut kaynak, &mut izgara, PNG_IMZASI.len())?;
        if izgara[..8] != PNG_IMZASI {
            return Err(Hata::PngImzasiBozuk);
        }
        izgara.drain(..8);

        // `IHDR` her zaman ilk blogtur ve 13 bayttir.
        izgara_doldur(&mut kaynak, &mut izgara, 25)?;
        let (uzunluk, tip) = basligi_ayir(&izgara, 0)?;
        if &tip != b"IHDR" {
            return Err(Hata::PngBaslikBozuk {
                ayrinti: format!("ilk blok IHDR olmali, bulunan {}", tip_metin(&tip)),
            });
        }
        if uzunluk != 13 {
            return Err(Hata::PngBaslikBozuk {
                ayrinti: format!("IHDR 13 bayt olmali, bulunan {uzunluk}"),
            });
        }
        let baslik = ihdr_ayir(&izgara[8..21])?;
        crc_denle(&izgara[..25], &tip)?;

        let mut akis = Self {
            kaynak,
            izgara: izgara[25..].to_vec(),
            tampon: Vec::new(),
            konum: 0,
            bitti: false,
            meta: Vec::new(),
            bloklar: vec!["IHDR".to_string()],
            palet: Vec::new(),
            trns: None,
            baslik,
        };
        // `IDAT` gelene kadar on-`IDAT` bloklarini isle.
        while akis.tampon.is_empty() && !akis.bitti {
            akis.blok_isle()?;
        }
        Ok(akis)
    }

    /// En fazla `n` bayt IDAT verisi ister (siparisle gelen bloklardan).
    ///
    /// # Hatalar
    ///
    /// Blok CRC'si bozuk veya dosya `IEND` oncesi bitirse hata doner.
    pub fn istenen(&mut self, n: usize) -> Result<Vec<u8>, Hata> {
        while self.konum >= self.tampon.len() {
            if self.bitti {
                return Ok(Vec::new());
            }
            self.tampon.clear();
            self.konum = 0;
            self.blok_isle()?;
        }
        let kalan = self.tampon.len() - self.konum;
        let al = kalan.min(n);
        let veri = self.tampon[self.konum..self.konum + al].to_vec();
        // Konum ilerletilmezse ayni baytlar her cagrida tekrar doner; inflate
        // akisi bu yuzden 32 KiB'ten buyuk IDAT'larda bozulur ve `IEND`'e
        // kadar ilerleme hicbir zaman gerceklesmez.
        self.konum += al;
        Ok(veri)
    }

    /// Siradaki blogu isler. `IDAT` disi bloklar `meta` tamponuna yazilir.
    fn blok_isle(&mut self) -> Result<(), Hata> {
        if self.izgara.len() < 8 {
            izgara_doldur(&mut self.kaynak, &mut self.izgara, 8)?;
        }
        let (uzunluk, tip) = basligi_ayir(&self.izgara, 0)?;
        let veri_uzunluk = uzunlugu_dogrula(&tip, uzunluk)?;
        let toplam = 12 + veri_uzunluk;
        if self.izgara.len() < toplam {
            izgara_doldur(&mut self.kaynak, &mut self.izgara, toplam)?;
        }
        let veri = self.izgara[8..8 + veri_uzunluk].to_vec();
        crc_denle(&self.izgara[..toplam], &tip)?;

        if &tip == b"IEND" {
            self.bloklar.push("IEND".to_string());
            self.bitti = true;
        } else if &tip == b"IDAT" {
            // `IDAT` de raporlanir: `info` ciktisinda piksel verisinin varligi
            // en belirleyici blok bilgisidir.
            self.bloklar.push("IDAT".to_string());
            self.tampon.extend_from_slice(&veri);
        } else if &tip == b"acTL" {
            return Err(Hata::PngAnimasyonluDesteklenmiyor);
        } else {
            self.bloklar.push(tip_metin(&tip));
            self.meta_ya_ekle(&tip, &veri)?;
            if &tip == b"PLTE" {
                self.paleti_guncelle(&veri);
            } else if &tip == b"tRNS" && self.trns.is_none() {
                self.trns = Some(veri);
            }
        }
        self.izgara.drain(..toplam);
        Ok(())
    }

    /// `meta` tamponunu sinirla birlikte buyutur.
    ///
    /// Kaydedilen bicim `uzunluk (4, big-endian) + tip (4) + veri` seklindedir;
    /// boylece ard arda gelen birden fazla `tEXt` blogu belirsizlik olmadan
    /// ayristirilabilir.
    fn meta_ya_ekle(&mut self, tip: &[u8; 4], veri: &[u8]) -> Result<(), Hata> {
        let yeni_toplam = self.meta.len() as u64 + 8 + veri.len() as u64;
        if yeni_toplam > crate::sinir::EN_FAZLA_META_SEGMENT {
            return Err(Hata::MetaVeriSinir {
                segment: tip_metin(tip),
                bayt: yeni_toplam,
                sinir: crate::sinir::EN_FAZLA_META_SEGMENT,
            });
        }
        self.meta
            .extend_from_slice(&(veri.len() as u32).to_be_bytes());
        self.meta.extend_from_slice(tip);
        self.meta.extend_from_slice(veri);
        Ok(())
    }

    /// `PLTE` blogunu 8-bit RGB ucusune cevirir.
    fn paleti_guncelle(&mut self, veri: &[u8]) {
        self.palet.clear();
        for ucu in veri.chunks_exact(3).take(256) {
            self.palet.push([ucu[0], ucu[1], ucu[2]]);
        }
    }

    /// `IDAT` disi bloklardan metin ve EXIF kayitlarini cikarir.
    #[must_use]
    pub fn meta_kayitlarini_ayikla(&self) -> (Vec<MetinKaydi>, Option<Vec<u8>>) {
        png_metin_ayikla(&self.meta)
    }
}

/// Bir blok tamponunun CRC'sini dogrular (`blok`: `len + tip + veri + crc`).
fn crc_denle(blok: &[u8], tip: &[u8; 4]) -> Result<(), Hata> {
    let veri_uzunluk = blok.len() - 12;
    let veri = &blok[8..8 + veri_uzunluk];
    let hesaplanan = crc32_tip_ile(tip, veri);
    let k = 8 + veri_uzunluk;
    let dosyadan = u32::from_be_bytes([blok[k], blok[k + 1], blok[k + 2], blok[k + 3]]);
    if dosyadan != hesaplanan {
        return Err(Hata::PngCrcBozuk {
            blok_tipi: tip_metin(tip),
            dosyadan,
            hesaplanan,
        });
    }
    Ok(())
}

/// Okuyucudan tam olarak gereken kadar bayt toplar (`izgara` en az `n` bayta
/// ulaşana dek).
///
/// **Neden yalnızca gereken kadar okunur?** Okuyucu akış hâlindedir; 32 KiB'lık
/// bir tamponla "hata bakmak için" önden okumak, sonraki blokları gereğinden
/// fazla tüketir ve blok sınırlarını kaydırır. Bu yüzden her adımda tam olarak
/// eksik bayt sayısı istenir.
///
/// # Hatalar
///
/// Dosya beklenenden erken biterse `Hata::PngIendYok` döner.
fn izgara_doldur<R: Read>(kaynak: &mut R, izgara: &mut Vec<u8>, n: usize) -> Result<(), Hata> {
    while izgara.len() < n {
        let bas = izgara.len();
        let eksik = n - bas;
        izgara.resize(bas + eksik, 0);
        let okunan = kaynak
            .read(&mut izgara[bas..])
            .map_err(|e| Hata::PngBlokBozuk {
                blok_tipi: "?".to_string(),
                ayrinti: format!("okuma hatasi: {e}"),
            })?;
        izgara.truncate(bas + okunan);
        if okunan == 0 {
            return Err(Hata::PngIendYok);
        }
    }
    Ok(())
}

/// Tam cozulmus PNG icerigi.
#[derive(Debug, Clone)]
pub struct PngIcerik {
    /// `IHDR` bilgileri.
    pub baslik: Baslik,
    /// 8-bit RGBA govde.
    pub govde: Raster,
    /// `PLTE` paleti (renk tipi 3 ise doludur).
    pub palet: Vec<[u8; 3]>,
    /// `tEXt` / `zTXt` / `iTXt` kayitlari.
    pub metin: Vec<MetinKaydi>,
    /// `eXIf` blogunun ham icerigi.
    pub exif: Option<Vec<u8>>,
    /// Dosyada gorulen blok tipleri (dosya sirasiyla).
    pub bloklar: Vec<String>,
}

/// Bir okuyucudan PNG cozer ve tam icerigi dondurur.
///
/// # Hatalar
///
/// Girdi PNG degilse, baslik gecersizse, CRC bozuksa, IDAT yetersizse veya
/// inflate basarisizsa uygun `Hata` doner.
pub fn png_coz<R: Read>(kaynak: R) -> Result<PngIcerik, Hata> {
    let (okuyucu, hata_yuvasi) = IdatOkuyucu::yeni(kaynak)?;
    let baslik = okuyucu.baslik();
    let palet = okuyucu.akis.palet.clone();
    let trns = okuyucu.akis.trns.clone();
    if baslik.renk_tipi == 3 && palet.is_empty() {
        return Err(Hata::PngBaslikBozuk {
            ayrinti: "renk tipi 3 (palet) ancak PLTE blogu yok".to_string(),
        });
    }
    let mut cozucu = flate2::read::ZlibDecoder::new(okuyucu);
    let govde = satirlari_coz(&mut cozucu, &baslik, &palet, &trns, &hata_yuvasi)?;
    // zlib akisini kapat ve altindaki PNG okuyucuyu geri al.
    let mut ic = cozucu.into_inner();
    // PNG spec'e gore dosya `IEND` ile bitmelidir; yarim dosya sessizce
    // basariyla kabul edilmez.
    ic.tamamini_yig()?;
    let (metin, exif) = ic.meta_kayitlari();
    Ok(PngIcerik {
        baslik,
        govde,
        palet,
        metin,
        exif,
        bloklar: ic.bloklar(),
    })
}

/// PNG dosyasini acar ve cozer.
///
/// # Hatalar
///
/// Dosya acilamazsa veya icerik gecersizse hata doner.
pub fn png_dosya_coz(yol: &Path) -> Result<PngIcerik, Hata> {
    let dosya = std::fs::File::open(yol).map_err(|e| io_hata(yol, e))?;
    let okuyucu = std::io::BufReader::with_capacity(64 * 1024, dosya);
    png_coz(okuyucu)
}

/// Adaptörün taşıdığı hata hücresinin paylaşılan tipi.
///
/// `IdatOkuyucu` bir `std::io::Read` uygular; blok/CRC hatası `io::Error` ile
/// taşınamadığı için hücrede saklanır ve çağıran taraf öncelikli hata olarak seçer.
pub type HataHucresi = Rc<Cell<Option<Hata>>>;

/// `IDAT` bloklarini `std::io::Read` olarak sunan adaptor.
///
/// Boylece inflate isini `flate2::read::ZlibDecoder` yapar; adaptor yalnizca
/// blok cercevelemesini cozer ve CRC'si dogrulanmis `IDAT` yuklerini siraya
/// koyar. Blok/CRC hatasi `io::Error` ile tasinamayacagi icin `Hata` ortak bir
/// hucrede saklanir ve cagiran taraf onu **oncelikli** hatasi olarak secer.
pub struct IdatOkuyucu<R: Read> {
    akis: IdatOkisi<R>,
    kalan: Vec<u8>,
    hata: HataHucresi,
}

impl<R: Read> IdatOkuyucu<R> {
    /// Adaptoru kurar; ayni hucreyi dondurur (hatayi okumak icin).
    ///
    /// # Hatalar
    ///
    /// Imza, `IHDR` veya oncesi bloklar gecersizse hata doner.
    pub fn yeni(kaynak: R) -> Result<(Self, HataHucresi), Hata> {
        let hata = Rc::new(Cell::new(None));
        let akis = IdatOkisi::yeni(kaynak)?;
        Ok((
            Self {
                akis,
                kalan: Vec::new(),
                hata: Rc::clone(&hata),
            },
            hata,
        ))
    }

    /// Okunmus `IHDR` bilgileri.
    #[must_use]
    pub fn baslik(&self) -> Baslik {
        self.akis.baslik
    }

    /// Dosyada gorulen blok tipleri.
    #[must_use]
    pub fn bloklar(&self) -> Vec<String> {
        self.akis.bloklar.clone()
    }

    /// `IDAT` disi bloklardan metin ve `eXIf` kayitlarini cikarir.
    #[must_use]
    pub fn meta_kayitlari(&self) -> (Vec<MetinKaydi>, Option<Vec<u8>>) {
        self.akis.meta_kayitlarini_ayikla()
    }

    /// Akisi `IEND` bloguna kadar ilerletir ve butunlugu dogrular.
    ///
    /// # Hatalar
    ///
    /// Dosya `IEND` ile bitmiyorsa `Hata::PngIendYok` doner.
    pub fn tamamini_yig(&mut self) -> Result<(), Hata> {
        while !self.akis.bitti {
            if self.akis.istenen(64 * 1024)?.is_empty() {
                break;
            }
        }
        if self.akis.bitti {
            Ok(())
        } else {
            Err(Hata::PngIendYok)
        }
    }

    /// Kaydedilmis hatayi `take` eder.
    #[must_use]
    pub fn alinan_hata(&self) -> Option<Hata> {
        self.hata.take()
    }
}

impl<R: Read> Read for IdatOkuyucu<R> {
    fn read(&mut self, tampon: &mut [u8]) -> std::io::Result<usize> {
        if self.kalan.is_empty() {
            match self.akis.istenen(32 * 1024) {
                Ok(veri) => self.kalan = veri,
                Err(hata) => {
                    self.hata.set(Some(hata));
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "PNG blok hatasi",
                    ));
                }
            }
        }
        let n = self.kalan.len().min(tampon.len());
        tampon[..n].copy_from_slice(&self.kalan[..n]);
        self.kalan.drain(..n);
        Ok(n)
    }
}

/// `IDAT` akisindan tum tarama satirlarini cozup RGBA govdeyi kurar.
///
/// Filtre geri alma `crate::png::filtre` ile, ornekleme genisletme asagida
/// yapilir. Bellek kullanimi: iki satir tamponu + cikti govdesi. Kaynak
/// dosyanin tamami hicbir zaman bellekte tutulmaz.
///
/// # Hatalar
///
/// Tarama satiri sayisi zlib akisindan az cozulursa `Hata::PngIdatYetersiz`,
/// akis bozuksa `Hata::PngZlibBozuk` doner.
fn satirlari_coz<R: Read>(
    cozucu: &mut flate2::read::ZlibDecoder<IdatOkuyucu<R>>,
    baslik: &Baslik,
    palet: &[[u8; 3]],
    trns: &Option<Vec<u8>>,
    hata_yuvasi: &HataHucresi,
) -> Result<Raster, Hata> {
    let satir_bayt = baslik.satir_bayt();
    let bpp = baslik.piksel_bayt();
    let mut govde = Raster::yeni_sifir(baslik.genislik, baslik.yukseklik)?;
    let mut onceki = vec![0u8; satir_bayt];
    let mut ham = vec![0u8; satir_bayt];
    let mut satir = vec![0u8; satir_bayt + 1];

    for y in 0..baslik.yukseklik {
        let okuma = cozucu.read_exact(&mut satir);
        if let Err(hata) = okuma {
            return Err(hata_yuvasi.take().unwrap_or(Hata::PngZlibBozuk {
                ayrinti: hata.to_string(),
            }));
        }
        if satir[0] > EN_FAZLA_FILTRE_TIPI {
            return Err(Hata::PngFiltreTipiGecersiz { tip: satir[0] });
        }
        filtreyi_geri_al(satir[0], bpp, &onceki, &satir[1..], &mut ham)?;
        satir_genislet(baslik, &ham, palet, trns, &mut govde, y);
        onceki.copy_from_slice(&ham);
    }
    Ok(govde)
}
/// Bir tarama satirini 8-bit RGBA'ya genisletip govdenin `y` satirina yazar.
fn satir_genislet(
    baslik: &Baslik,
    ham: &[u8],
    palet: &[[u8; 3]],
    trns: &Option<Vec<u8>>,
    govde: &mut Raster,
    y: u32,
) {
    let mut ofset = 0usize;
    for x in 0..baslik.genislik {
        let piksel = piksel_ornegi(baslik, ham, palet, trns, &mut ofset);
        govde.piksel_ata(x, y, piksel);
    }
}

/// Siradaki pikselin RGBA degerini cozer ve `ofset`i ilerletir.
///
/// 16-bit orneklerde yalnizca **yuksek bayt** kullanilir (8-bit'e indirgeme);
/// 1/2/4-bit ornekler 8-bit araliga esit dagilimla gerilenir
/// (`v * 255 / (2^d - 1)`), boylece siyah ve beyaz tam olarak korunur.
fn piksel_ornegi(
    baslik: &Baslik,
    ham: &[u8],
    palet: &[[u8; 3]],
    trns: &Option<Vec<u8>>,
    ofset: &mut usize,
) -> [u8; KANAL] {
    let derinlik = u32::from(baslik.bit_derinligi);
    let kanal = baslik.kanal_sayisi().unwrap_or(1);
    let mut ornek = [0u16; 4];
    for n in ornek.iter_mut().take(usize::from(kanal)) {
        *n = ornge_al(ham, ofset, derinlik);
    }
    match baslik.renk_tipi {
        0 => {
            let gri = ornek_8(ornek[0], derinlik);
            let mut p = [gri, gri, gri, 255];
            if trns_ayirt_ediyor(trns, &[ornek[0]]) {
                p[3] = 0;
            }
            p
        }
        2 => {
            let mut p = [
                ornek_8(ornek[0], derinlik),
                ornek_8(ornek[1], derinlik),
                ornek_8(ornek[2], derinlik),
                255,
            ];
            if trns_ayirt_ediyor(trns, &[ornek[0], ornek[1], ornek[2]]) {
                p[3] = 0;
            }
            p
        }
        3 => {
            let indeks = usize::from(ornek[0]);
            let renk = palet.get(indeks).copied().unwrap_or([0, 0, 0]);
            let a = trns
                .as_ref()
                .and_then(|t| t.get(indeks))
                .copied()
                .unwrap_or(255);
            [renk[0], renk[1], renk[2], a]
        }
        4 => {
            let gri = ornek_8(ornek[0], derinlik);
            let mut p = [gri, gri, gri, ornek_8(ornek[1], derinlik)];
            if trns_ayirt_ediyor(trns, &[ornek[0]]) {
                p[3] = 0;
            }
            p
        }
        _ => [
            ornek_8(ornek[0], derinlik),
            ornek_8(ornek[1], derinlik),
            ornek_8(ornek[2], derinlik),
            ornek_8(ornek[3], derinlik),
        ],
    }
}

/// Bir örneği 8-bit kanal değerine çevirir.
///
/// 16-bit örneklerde yalnızca **yüksek bayt** kullanılır (PNG'nin 8-bit
/// çıktısında tam hassasiyet korunamaz). 1/2/4/8-bit örnekler zaten
/// `ornge_al` içinde 0..=255 aralığına gerilendiği için doğrudan alınır.
fn ornek_8(deger: u16, derinlik: u32) -> u8 {
    if derinlik == 16 {
        (deger >> 8) as u8
    } else {
        (deger & 0xFF) as u8
    }
}

/// 16-bit bir ornegin yuksek baytini 8-bit degerine cevirir (yalnizca 16-bit
/// derinlikte kullanilir; testler icin).
#[cfg(test)]
fn yuksek_bayt(deger: u16) -> u8 {
    (deger >> 8) as u8
}

/// `tRNS` saydamlik anahtari, verilen orneklerle eslesiyor mu diye bakar.
fn trns_ayirt_ediyor(trns: &Option<Vec<u8>>, ornekler: &[u16]) -> bool {
    match trns {
        Some(t) if t.len() >= 2 * ornekler.len() => ornekler
            .iter()
            .enumerate()
            .all(|(i, &o)| u16::from_be_bytes([t[2 * i], t[2 * i + 1]]) == o),
        _ => false,
    }
}

/// Bir tarama satirindan siradaki `ornek`i okur ve `ofset`i **bit** cinsinden
/// ilerletir.
///
/// 1, 2, 4 bit derinliklerinde ornekler bayt icinde **ustten alta** (MSB once)
/// dizilir (PNG spec, bolum 7.2) ve pikseller bayt hizasinda degildir; bu
/// yuzden ofset bit sayilidir ve tum satir boyunca artar. 8 ve 16 bit
/// derinliklerinde birer bayt / iki bayt tuketilir.
fn ornge_al(ham: &[u8], ofset: &mut usize, derinlik: u32) -> u16 {
    let konum = *ofset;
    let i = konum / 8;
    let bayt = ham.get(i).copied().unwrap_or(0);
    match derinlik {
        16 => {
            *ofset += 16;
            u16::from_be_bytes([bayt, ham.get(i + 1).copied().unwrap_or(0)])
        }
        8 => {
            *ofset += 8;
            u16::from(bayt)
        }
        4 => {
            *ofset += 4;
            let ham_deger = if konum % 8 == 0 {
                bayt >> 4
            } else {
                bayt & 0x0F
            };
            u16::from(ham_deger * 0x11)
        }
        2 => {
            *ofset += 2;
            let kay = (konum % 8) / 2;
            u16::from(((bayt >> (6 - 2 * kay)) & 0x03) * 0x55)
        }
        _ => {
            *ofset += 1;
            u16::from(((bayt >> (7 - (konum % 8))) & 0x01) * 0xFF)
        }
    }
}

/// Yalnizca basligi okur; pikselleri cozmez.
///
/// `pixelmill info` komutu icindir: 40 MB'lık bir PNG'de pikselleri acmadan
/// boyut, renk tipi ve blok listesi bildirilir.
///
/// # Hatalar
///
/// Girdi PNG degilse veya bloklar bozuksa hata doner.
pub fn png_bilgi<R: Read>(kaynak: R) -> Result<BaslikBilgi, Hata> {
    let (mut okuyucu, _) = IdatOkuyucu::yeni(kaynak)?;
    let baslik = okuyucu.baslik();
    // IDAT verisini at; blok sinifini ve meta veriyi topla.
    okuyucu.tamamini_yig()?;
    let palet_uzunlugu = okuyucu.akis.palet.len();
    let bloklar = okuyucu.bloklar();
    let (metin, exif) = okuyucu.meta_kayitlari();
    Ok(BaslikBilgi {
        baslik,
        palet_uzunlugu,
        bloklar,
        metin,
        exif,
    })
}

/// Pikselleri acmadan elde edilen PNG bilgisi.
#[derive(Debug, Clone)]
pub struct BaslikBilgi {
    /// `IHDR` bilgileri.
    pub baslik: Baslik,
    /// `PLTE` giris sayisi.
    pub palet_uzunlugu: usize,
    /// Dosyada gorulen blok tipleri.
    pub bloklar: Vec<String>,
    /// Metin kayitlari.
    pub metin: Vec<MetinKaydi>,
    /// `eXIf` blogu, varsa.
    pub exif: Option<Vec<u8>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png::yazma::{PngSecenek, PngYazici};
    use crate::test_yardimci::ac;

    fn ornek_baslik() -> Baslik {
        Baslik {
            genislik: 2,
            yukseklik: 2,
            bit_derinligi: 8,
            renk_tipi: 6,
            sikistirma: 0,
            filtre_yontemi: 0,
            sekme: 0,
        }
    }

    /// Test icin tam bir PNG uretir (dort kenar yazma + bitirme).
    fn png_uret(
        genislik: u32,
        yukseklik: u32,
        bit_derinligi: u8,
        renk_tipi: u8,
        govde: &Raster,
    ) -> Vec<u8> {
        let secenek = ac(PngSecenek::yeni(
            genislik,
            yukseklik,
            bit_derinligi,
            renk_tipi,
        ));
        let mut baytlar = Vec::new();
        let yazici = ac(PngYazici::yeni(&mut baytlar, secenek));
        let mut w = yazici;
        for y in 0..yukseklik {
            ac(w.yaz(&kutu_doldur(govde, y, renk_tipi, bit_derinligi)));
        }
        ac(w.bitir());
        baytlar
    }

    /// Bir govde satirini hedef renk tipine ve bit derinligine gore paketler.
    fn kutu_doldur(govde: &Raster, y: u32, renk_tipi: u8, bit_derinligi: u8) -> Vec<u8> {
        match (renk_tipi, bit_derinligi) {
            (0, 8) => (0..govde.genislik).map(|x| govde.piksel(x, y)[0]).collect(),
            (2, 8) => (0..govde.genislik)
                .flat_map(|x| {
                    let p = govde.piksel(x, y);
                    [p[0], p[1], p[2]]
                })
                .collect(),
            (4, 8) => (0..govde.genislik)
                .flat_map(|x| {
                    let p = govde.piksel(x, y);
                    [p[0], p[3]]
                })
                .collect(),
            (6, 8) => (0..govde.genislik)
                .flat_map(|x| govde.piksel(x, y))
                .collect(),
            (0, 16) => (0..govde.genislik)
                .flat_map(|x| {
                    let v = govde.piksel(x, y)[0];
                    // PNG 16-bit ornekleri big-endian'dir.
                    [v, 0u8]
                })
                .collect(),
            _ => panic!("test yardimcisi bu bicimi desteklemiyor"),
        }
    }

    fn dolu(g: &mut Raster, uret: impl Fn(u32, u32) -> [u8; 4]) {
        for y in 0..g.yukseklik {
            for x in 0..g.genislik {
                g.piksel_ata(x, y, uret(x, y));
            }
        }
    }

    #[test]
    fn kanal_sayilari_spektle_uyumlu() {
        for (renk_tipi, beklenen) in [(0u8, 1u8), (2, 3), (3, 1), (4, 2), (6, 4)] {
            let b = Baslik {
                renk_tipi,
                ..ornek_baslik()
            };
            if b.kanal_sayisi() != Some(beklenen) {
                panic!("renk tipi {renk_tipi} icin kanal sayisi {beklenen} olmali");
            }
        }
        let gecersiz = Baslik {
            renk_tipi: 1,
            ..ornek_baslik()
        };
        if gecersiz.kanal_sayisi().is_some() {
            panic!("renk tipi 1 tanimsiz olmali");
        }
    }

    #[test]
    fn satir_bayt_yuvarlamali_hesaplanir() {
        let b = Baslik {
            genislik: 3,
            renk_tipi: 0,
            bit_derinligi: 1,
            ..ornek_baslik()
        };
        if b.satir_bayt() != 1 {
            panic!("3 piksel x 1 bit = 4 bit -> 1 bayt olmali");
        }
        let b2 = Baslik {
            genislik: 9,
            renk_tipi: 0,
            bit_derinligi: 1,
            ..ornek_baslik()
        };
        if b2.satir_bayt() != 2 {
            panic!("9 piksel x 1 bit = 9 bit -> 2 bayt olmali");
        }
        let b3 = Baslik {
            genislik: 2,
            renk_tipi: 6,
            bit_derinligi: 8,
            ..ornek_baslik()
        };
        if b3.satir_bayt() != 8 || b3.piksel_bayt() != 4 {
            panic!("2x RGBA8 satir 8 bayt, bpp 4 olmali");
        }
    }

    #[test]
    fn sekmeli_png_acikca_reddedilir() {
        match basligi_dogrula(&Baslik {
            sekme: 1,
            ..ornek_baslik()
        }) {
            Err(Hata::PngSekmeliDesteklenmiyor { yontem: 1 }) => {}
            diger => panic!("Adam7 reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn gecersiz_kombinasyonlar_reddedilir() {
        for (renk_tipi, derinlik) in [(0u8, 3u8), (2, 4), (3, 16), (6, 1), (1, 8)] {
            let b = Baslik {
                renk_tipi,
                bit_derinligi: derinlik,
                ..ornek_baslik()
            };
            if basligi_dogrula(&b).is_ok() {
                panic!("renk tipi {renk_tipi} / derinlik {derinlik} reddedilmeliydi");
            }
        }
    }

    #[test]
    fn sikistirma_ve_filtre_yontemi_sifir_olmali() {
        for alan in 0..2 {
            let mut b = ornek_baslik();
            if alan == 0 {
                b.sikistirma = 1;
            } else {
                b.filtre_yontemi = 1;
            }
            if basligi_dogrula(&b).is_ok() {
                panic!("alan {alan} icin hata bekleniyordu");
            }
        }
    }

    #[test]
    fn ihdr_kisa_olunca_hata_dondurur() {
        match ihdr_ayir(&[0u8; 12]) {
            Err(Hata::PngBaslikBozuk { .. }) => {}
            diger => panic!("12 baytlik IHDR reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn ihdr_ayristirma_dogru() {
        let mut veri = Vec::new();
        veri.extend_from_slice(&300u32.to_be_bytes());
        veri.extend_from_slice(&200u32.to_be_bytes());
        veri.extend_from_slice(&[8, 6, 0, 0, 0]);
        let b = ac(ihdr_ayir(&veri));
        if b.genislik != 300 || b.yukseklik != 200 || b.bit_derinligi != 8 || b.renk_tipi != 6 {
            panic!("IHDR ayristirilamadi: {b:?}");
        }
        if b.renk_tipi_adi().is_empty() {
            panic!("renk tipi adi bos olmali");
        }
    }

    #[test]
    fn rgba_gidis_donus_basar() {
        let mut g = ac(Raster::yeni_sifir(4, 3));
        dolu(&mut g, |x, y| [(x * 30) as u8, (y * 40) as u8, 7, 200]);
        let baytlar = png_uret(4, 3, 8, 6, &g);
        let okunan = ac(png_coz(&baytlar[..]));
        if okunan.govde != g {
            panic!("RGBA PNG gidis-donus piksel farki var");
        }
    }

    #[test]
    fn gri_png_gidis_donus_basar() {
        let mut g = ac(Raster::yeni_sifir(3, 2));
        dolu(&mut g, |x, y| {
            let v = ((x + y) * 30) as u8;
            [v, v, v, 255]
        });
        let baytlar = png_uret(3, 2, 8, 0, &g);
        let okunan = ac(png_coz(&baytlar[..]));
        if okunan.baslik.renk_tipi != 0 {
            panic!("gri renk tipi korunmadi");
        }
        if okunan.govde != g {
            panic!("gri PNG gidis-donus farkli");
        }
    }

    #[test]
    fn rgb_png_alfa_kaybolur_ve_geri_donus_dogru() {
        let mut g = ac(Raster::yeni_sifir(2, 2));
        dolu(&mut g, |x, _| [x as u8 * 10, 5, 200, 255]);
        let baytlar = png_uret(2, 2, 8, 2, &g);
        let okunan = ac(png_coz(&baytlar[..]));
        for y in 0..2u32 {
            for x in 0..2u32 {
                if okunan.govde.piksel(x, y)[3] != 255 {
                    panic!("RGB ciktida alfa 255 olmali");
                }
            }
        }
    }

    #[test]
    fn on_alti_bit_yuksek_bayta_indirgenir() {
        let mut g = ac(Raster::yeni_sifir(2, 1));
        g.piksel_ata(0, 0, [0x12, 0x12, 0x12, 255]);
        g.piksel_ata(1, 0, [0xAB, 0xAB, 0xAB, 255]);
        let baytlar = png_uret(2, 1, 16, 0, &g);
        let okunan = ac(png_coz(&baytlar[..]));
        if okunan.govde.piksel(0, 0)[0] != 0x12 || okunan.govde.piksel(1, 0)[0] != 0xAB {
            panic!("16-bit indirgeme yanlis");
        }
    }

    #[test]
    fn tek_piksel_govde_calisir() {
        let mut g = ac(Raster::yeni_sifir(1, 1));
        g.piksel_ata(0, 0, [1, 2, 3, 255]);
        let baytlar = png_uret(1, 1, 8, 6, &g);
        let okunan = ac(png_coz(&baytlar[..]));
        if okunan.govde.piksel(0, 0) != [1, 2, 3, 255] {
            panic!("1x1 gidis-donus basarisiz");
        }
    }

    #[test]
    fn sifir_boyutlu_baslik_reddedilir() {
        let mut veri = Vec::new();
        veri.extend_from_slice(&0u32.to_be_bytes());
        veri.extend_from_slice(&0u32.to_be_bytes());
        veri.extend_from_slice(&[8, 6, 0, 0, 0]);
        match ihdr_ayir(&veri) {
            Err(Hata::PngBoyutGecersiz { .. }) => {}
            diger => panic!("0x0 reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn imza_bozuk_hata_dondurur() {
        match png_coz(&[0u8; 32][..]) {
            Err(Hata::PngImzasiBozuk) => {}
            diger => panic!("bozuk imza hata donmeli: {diger:?}"),
        }
    }

    #[test]
    fn bozuk_crc_hata_dondurur() {
        let mut g = ac(Raster::yeni_sifir(2, 2));
        dolu(&mut g, |x, y| [x as u8, y as u8, 0, 255]);
        let baytlar = png_uret(2, 2, 8, 6, &g);
        // IDAT verisinin bir baytini boz; CRC artik tutmaz.
        let mut bozuk = baytlar;
        let idat = match bozuk.windows(4).position(|p| p == b"IDAT") {
            Some(i) => i,
            None => panic!("IDAT blogu bulunamadi"),
        };
        bozuk[idat + 9] ^= 0xFF;
        match png_coz(&bozuk[..]) {
            Err(Hata::PngCrcBozuk { .. }) => {}
            diger => panic!("bozuk CRC reddedilmeliydi: {diger:?}"),
        }
    }

    #[test]
    fn yarim_kalmis_dosya_hata_dondurur() {
        let mut g = ac(Raster::yeni_sifir(2, 2));
        dolu(&mut g, |x, y| [x as u8, y as u8, 0, 255]);
        let baytlar = png_uret(2, 2, 8, 6, &g);
        let kesik = &baytlar[..baytlar.len() - 12];
        match png_coz(kesik) {
            Err(Hata::PngIendYok)
            | Err(Hata::PngIdatYetersiz { .. })
            | Err(Hata::PngZlibBozuk { .. }) => {}
            diger => panic!("yarim dosya hata donmeliydi: {diger:?}"),
        }
    }

    #[test]
    fn ornge_al_bayt_duzeni_dogru() {
        let mut o = 0;
        if ornge_al(&[1, 2, 3], &mut o, 8) != 1 || o != 8 {
            panic!("8-bit ornge alma hatali");
        }
        let mut o = 0;
        if ornge_al(&[0x12, 0x34], &mut o, 16) != 0x1234 || o != 16 {
            panic!("16-bit ornge alma hatali");
        }
        let mut o = 0;
        if ornge_al(&[0x80], &mut o, 1) != 255 {
            panic!("1-bit ilk ornek 255 olmali");
        }
        for _ in 0..7 {
            if ornge_al(&[0x80], &mut o, 1) != 0 {
                panic!("1-bit kalan ornekler 0 olmali");
            }
        }
        if o != 8 {
            panic!("8 adet 1-bit ornek bir bayt tuketmeli, ofset={o}");
        }
        let mut o = 0;
        if ornge_al(&[0xAF], &mut o, 4) != 0xAA || ornge_al(&[0xAF], &mut o, 4) != 0xFF {
            panic!("4-bit ornge alma hatali");
        }
        if o != 8 {
            panic!("4-bit iki ornek bir bayt tuketmeli");
        }
        let mut o = 0;
        for beklenen in [3u16, 2, 1, 0] {
            if ornge_al(&[0xE4], &mut o, 2) != beklenen * 0x55 {
                panic!("2-bit ornge alma hatali");
            }
        }
        if o != 8 {
            panic!("2-bit dort ornek bir bayt tuketmeli");
        }
    }

    #[test]
    fn ornge_al_kisa_tamponda_tasma_yapmaz() {
        let mut o = 0;
        let _ = ornge_al(&[], &mut o, 8);
        let _ = ornge_al(&[1], &mut o, 16);
        let _ = ornge_al(&[1], &mut o, 4);
        let _ = ornge_al(&[1], &mut o, 2);
        let _ = ornge_al(&[1], &mut o, 1);
    }

    #[test]
    fn trns_ayirt_edici_dogru() {
        let t = Some(vec![0x00, 0x10]);
        if !trns_ayirt_ediyor(&t, &[0x0010]) {
            panic!("eslesen saydamlik anahtari algilanmali");
        }
        if trns_ayirt_ediyor(&t, &[0x0020]) {
            panic!("eslesmeyen anahtar saydam sayilmamali");
        }
        if trns_ayirt_ediyor(&None, &[0x0010]) {
            panic!("tRNS yoksa saydamlik olmamali");
        }
        if trns_ayirt_ediyor(&Some(vec![0x00]), &[0x0010]) {
            panic!("kisa tRNS dikkate alinmamali");
        }
    }

    #[test]
    fn yuksek_bayt_indirgeme_yapar() {
        if yuksek_bayt(0xABCD) != 0xAB {
            panic!("16-bit degerin yuksek bayti alinmali");
        }
    }

    #[test]
    fn bilgi_okuma_piksel_acmadan_calisir() {
        let mut g = ac(Raster::yeni_sifir(3, 2));
        dolu(&mut g, |x, y| [x as u8, y as u8, 0, 255]);
        let baytlar = png_uret(3, 2, 8, 6, &g);
        let bilgi = ac(png_bilgi(&baytlar[..]));
        if bilgi.baslik.genislik != 3 || bilgi.baslik.yukseklik != 2 {
            panic!("bilgi okuma basligi yanlis");
        }
        if !bilgi.bloklar.iter().any(|b| b == "IDAT") {
            panic!("IDAT raporda gorunmeli: {:?}", bilgi.bloklar);
        }
        if !bilgi.bloklar.iter().any(|b| b == "IEND") {
            panic!("IEND raporda gorunmeli: {:?}", bilgi.bloklar);
        }
    }

    #[test]
    fn acilmayan_dosya_yol_hatasi_dondurur() {
        match png_dosya_coz(Path::new("bu-dosya-yok-1234.png")) {
            Err(Hata::Dosya { .. }) => {}
            diger => panic!("acma hatasi bekleniyordu: {diger:?}"),
        }
    }
}
