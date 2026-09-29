//! Piksel tamponu ve satır akisi soyutlameleri.
//!
//! Bu modul "govde" veri yapisini ve onu **satur** olarak okumak icin kullanilan
//! [`SatirOkuyucu`] trait'ini tanimlar. Govde her zaman 8-bit RGBA seklindedir
//! (kanal sayisi 4); PNG'nin gri/palette/16-bit varyantlari okuma sirasinda
//! bu bicime **genisletilir**. Boylece yeniden boyutlandirma ve kuantizasyon
//! kodlari tek bir piksel duzeniyle calisir.
//!
//! Bellek notu: `Raster` tek kareyi bellekte tutar (raporun "Veri modeli"
//! karari: "yalnizca tek kare yuksek cozunurlukte bellekte tutulur").
//! PNG *dosyasi* ise hicbir zaman tam olarak okunmaz; bkz. [`crate::png::okuma`].

use crate::hata::Hata;

/// Bir kanaldaki ogrenek sayisi (PNG spec: 8-bit kanal).
pub const KANAL: usize = 4;

/// R, G, B, A kanallarinin tam sayisal indisleri.
pub const R_KANAL: usize = 0;
/// R, G, B, A kanallarinin tam sayisal indisleri.
pub const G_KANAL: usize = 1;
/// R, G, B, A kanallarinin tam sayisal indisleri.
pub const B_KANAL: usize = 2;
/// R, G, B, A kanallarinin tam sayisal indisleri.
pub const A_KANAL: usize = 3;

/// Tek kareyi tutan 8-bit RGBA govde.
///
/// Alanlar sirasiyla genislik, yukseklik ve piksel baytlari (satir satir,
/// her satir `genislik * 4` bayt) olarak duzenlidir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    /// Piksel genisligi.
    pub genislik: u32,
    /// Piksel yuksekligi.
    pub yukseklik: u32,
    /// RGBA baytlari, satir major duzende.
    pub piksel: Vec<u8>,
}

impl Raster {
    /// Verilen boyutta, tamamen saydam (alfa 0) bir govde olusturur.
    ///
    /// # Hatalar
    ///
    /// `genislik * yukseklik * 4` tasma yaratirsa veya sinir asilirsa
    /// [`Hata::PngBoyutGecersiz`] doner.
    pub fn yeni_sifir(genislik: u32, yukseklik: u32) -> Result<Self, Hata> {
        boyut_dogrula(genislik, u64::from(yukseklik))?;
        let piksel = vec![0u8; piksel_sayisi(genislik, yukseklik)];
        Ok(Self {
            genislik,
            yukseklik,
            piksel,
        })
    }

    /// Tek bir pikseli getirir.
    ///
    /// # Panikler
    ///
    /// Koordinatlar cerceve disindaysa panikler. Bu bir **programci hatasi**
    /// yoludur; kullanici girdisinden kaynaklanan her durum `Result` ile doner.
    #[must_use]
    pub fn piksel(&self, x: u32, y: u32) -> [u8; KANAL] {
        let ofset = self.ofset(x, y);
        let p = &self.piksel[ofset..ofset + KANAL];
        [p[0], p[1], p[2], p[3]]
    }

    /// Tek bir pikseli degistirir.
    ///
    /// # Panikler
    ///
    /// Koordinatlar cerceve disindaysa panikler (bkz. [`Raster::piksel`]).
    pub fn piksel_ata(&mut self, x: u32, y: u32, renk: [u8; KANAL]) {
        let ofset = self.ofset(x, y);
        self.piksel[ofset..ofset + KANAL].copy_from_slice(&renk);
    }

    /// `(x, y)` koordinatinin bayt ofseti.
    fn ofset(&self, x: u32, y: u32) -> usize {
        debug_assert!(x < self.genislik, "x cerceve disinda: {x}");
        debug_assert!(y < self.yukseklik, "y cerceve disinda: {y}");
        (y as usize) * (self.genislik as usize) * KANAL + (x as usize) * KANAL
    }

    /// Bir satirin kopyasini dondurur (RGBA, `genislik * 4` bayt).
    #[must_use]
    pub fn satir(&self, y: u32) -> Vec<u8> {
        let bas = self.ofset(0, y);
        let uz = self.genislik as usize * KANAL;
        self.piksel[bas..bas + uz].to_vec()
    }

    /// `y` numaralı satırı verilen baytlarla **üzerine yazar**.
    ///
    /// Çözümleme ve yeniden boyutlandırma çıktıyı satır satır doldurur; gövde
    /// `Raster::yeni_sifir` ile önceden ayrılmış olduğundan satırlar sona
    /// **eklenmez**, yerine yazılır.
    ///
    /// # Panikler
    ///
    /// Satır indeksi çerçeve dışındaysa veya uzunluk beklenenden farklıysa
    /// panikler. Bu bir programcı hatası yoludur.
    pub(crate) fn satir_ata(&mut self, y: u32, satir: &[u8]) {
        debug_assert!(y < self.yukseklik, "y cerceve disinda: {y}");
        let bas = self.ofset(0, y);
        let uz = self.genislik as usize * KANAL;
        self.piksel[bas..bas + uz].copy_from_slice(&satir[..uz]);
    }

    /// Govdenin tamamina sabit bir alfa degeri yazar.
    pub fn alfa_ata(&mut self, alfa: u8) {
        for p in self.piksel.iter_mut().skip(A_KANAL).step_by(KANAL) {
            *p = alfa;
        }
    }

    /// Herhangi bir pikselde alfa 255 disi deger var mi diye bakar.
    #[must_use]
    pub fn saydamlik_var(&self) -> bool {
        self.piksel
            .iter()
            .skip(A_KANAL)
            .step_by(KANAL)
            .any(|&a| a != 255)
    }

    /// Tum piksellerde R == G == B esit mi diye bakar (gri resmi tespiti).
    ///
    /// Alfa dikkate alinmaz; renk esitligi yeterlidir.
    #[must_use]
    pub fn gri_mi(&self) -> bool {
        self.piksel
            .chunks_exact(KANAL)
            .all(|p| p[0] == p[1] && p[1] == p[2])
    }

    /// Govdeyi belirtilen boyutlara **en yakin komsu** ile yeniden boyutlandirir.
    ///
    /// Bu kirpma/kenar boslugu sonrasi hedef boyuta tam uydurmak icin kullanilir;
    /// kalite icin gerekmez, bu yuzden bicubic degil bilineer uygulanir.
    ///
    /// # Hatalar
    ///
    /// Hedef boyut sifir veya sinir asiyorsa hata doner.
    pub fn hedefe_sikistir(&self, hedef_genislik: u32, hedef_yukseklik: u32) -> Result<Self, Hata> {
        let mut kaynak_g = self.genislik;
        let mut kaynak_y = self.yukseklik;
        while kaynak_g > hedef_genislik && kaynak_y > hedef_yukseklik {
            kaynak_g = (kaynak_g * 3 / 4).max(1);
            kaynak_y = (kaynak_y * 3 / 4).max(1);
        }
        crate::boyut::yeniden_boyutlandir(
            &mut satir_okuyucu(self),
            self.genislik,
            self.yukseklik,
            kaynak_g,
            kaynak_y,
            crate::boyut::Filtre::Bilinear,
        )
    }
}

/// Bir govdeyi **satur sirasiyla** okumak icin trait.
///
/// Yeniden boyutlandirma yalnizca bu trait'i gorur; boylece ayni kod hem bellekteki
/// bir `Raster`i hem de dogrudan PNG dosyasindan akan satirlari islemekle kalir.
pub trait SatirOkuyucu {
    /// Siradaki satiri dondurur; veri tukendiginde `Ok(None)`.
    ///
    /// Dondurilen vektor RGBA duzenindedir ve `genislik * 4` bayt uzunlugundadir.
    fn sonraki(&mut self) -> Result<Option<Vec<u8>>, Hata>;

    /// Okunacak toplam satir sayisi (ileri zemin bilgisi).
    fn satir_sayisi(&self) -> u64;

    /// Bir satirin bayt uzunlugu (`genislik * 4`).
    fn satir_uzunlugu(&self) -> usize;
}

/// Bellekteki `Raster` uzerinden akis saglayan sarmalayici.
pub struct RasterOkuyucu<'a> {
    kaynak: &'a Raster,
    sira: u32,
}

impl<'a> RasterOkuyucu<'a> {
    /// Yeni bir okuyucu olusturur ve akisi bastan baslatir.
    pub fn yeni(kaynak: &'a Raster) -> Self {
        Self { kaynak, sira: 0 }
    }
}

impl SatirOkuyucu for RasterOkuyucu<'_> {
    fn sonraki(&mut self) -> Result<Option<Vec<u8>>, Hata> {
        if self.sira >= self.kaynak.yukseklik {
            return Ok(None);
        }
        let satir = self.kaynak.satir(self.sira);
        self.sira += 1;
        Ok(Some(satir))
    }

    fn satir_sayisi(&self) -> u64 {
        u64::from(self.kaynak.yukseklik)
    }

    fn satir_uzunlugu(&self) -> usize {
        self.kaynak.genislik as usize * KANAL
    }
}

/// Kolaylik saglayan baglayici: bir `Raster`i [`SatirOkuyucu`] haline getirir.
///
/// `Raster` dogrudan `SatirOkuyucu` **degildir** (her cagri yeni bir vektor
/// dondurur ve veri tukenince `None` verir), bu yuzden bu sarmalayici kullanilir.
pub(crate) fn satir_okuyucu(r: &Raster) -> RasterOkuyucu<'_> {
    RasterOkuyucu::yeni(r)
}

/// Bir govdenin piksel sayisini hesaplar (tasma denetimi ile).
pub(crate) fn piksel_sayisi(genislik: u32, yukseklik: u32) -> usize {
    let g = u64::from(genislik);
    let y = u64::from(yukseklik);
    let toplam: Option<usize> = g
        .checked_mul(y)
        .and_then(|v| v.checked_mul(KANAL as u64))
        .and_then(|v| usize::try_from(v).ok());
    toplam.unwrap_or_default()
}

/// Genislik/yukseklik degerlerini sinir ve sifir kurallarina karsi dogrular.
pub(crate) fn boyut_dogrula(genislik: u32, yukseklik: u64) -> Result<(), Hata> {
    let sebep = if genislik == 0 || yukseklik == 0 {
        "genislik ve yukseklik sifir olamaz".to_string()
    } else if genislik > crate::sinir::EN_FAZLA_KENAR
        || yukseklik > u64::from(crate::sinir::EN_FAZLA_KENAR)
    {
        format!("kenar siniri {} pikseli asti", crate::sinir::EN_FAZLA_KENAR)
    } else if u64::from(genislik)
        .checked_mul(yukseklik)
        .map_or(true, |v| v > crate::sinir::EN_FAZLA_PIKSEL)
    {
        format!(
            "toplam piksel siniri {} asildi",
            crate::sinir::EN_FAZLA_PIKSEL
        )
    } else {
        return Ok(());
    };
    Err(Hata::PngBoyutGecersiz {
        genislik: u64::from(genislik),
        yukseklik,
        sebep,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_yardimci::ac;

    fn ornek(genislik: u32, yukseklik: u32) -> Raster {
        ac(Raster::yeni_sifir(genislik, yukseklik))
    }

    #[test]
    fn sifir_govde_tam_saydam() {
        let r = ornek(2, 2);
        if r.piksel != vec![0u8; 16] {
            panic!("yeni govde sifir olmali");
        }
        if !r.saydamlik_var() {
            panic!("alfa 0 olan govde saydam sayilmali");
        }
    }

    #[test]
    fn bir_bir_piksel_konumlari_dogru() {
        let mut r = ornek(3, 2);
        r.piksel_ata(2, 1, [10, 20, 30, 40]);
        let okunan = r.piksel(2, 1);
        if okunan != [10, 20, 30, 40] {
            panic!("piksel girilmedi: {okunan:?}");
        }
        if r.piksel(0, 0) != [0, 0, 0, 0] {
            panic!("farkli konum kirpilmis olmali");
        }
    }

    #[test]
    fn satir_kopyasi_yalniz_ilgili_satiri_verir() {
        let mut r = ornek(2, 3);
        r.piksel_ata(0, 2, [1, 2, 3, 4]);
        let satir = r.satir(2);
        if satir.len() != 2 * KANAL {
            panic!("satir uzunlugu yanlis: {}", satir.len());
        }
        if satir[0..4] != [1, 2, 3, 4] {
            panic!("satir icerigi yanlis: {satir:?}");
        }
    }

    #[test]
    fn gri_tespiti_renk_esitligine_bakar() {
        let mut gri = ornek(2, 2);
        for x in 0..2u32 {
            for y in 0..2u32 {
                gri.piksel_ata(x, y, [77, 77, 77, 255]);
            }
        }
        if !gri.gri_mi() {
            panic!("R==G==B olan govde gri sayilmali");
        }
        gri.piksel_ata(0, 0, [77, 78, 77, 255]);
        if gri.gri_mi() {
            panic!("kanallari farkli olan govde gri sayilmamali");
        }
    }

    #[test]
    fn alfa_ata_tum_piksellere_uygulanir() {
        let mut r = ornek(2, 2);
        r.alfa_ata(255);
        if r.saydamlik_var() {
            panic!("alfa 255 iken saydamlik olmamali");
        }
        r.alfa_ata(0);
        if !r.saydamlik_var() {
            panic!("alfa 0 iken saydamlik olmali");
        }
    }

    #[test]
    fn sifir_boyut_reddedilir() {
        match Raster::yeni_sifir(0, 5) {
            Err(Hata::PngBoyutGecersiz { genislik: 0, .. }) => {}
            diger => panic!("sifir genislik reddedilmeli, alindi: {diger:?}"),
        }
        match Raster::yeni_sifir(5, 0) {
            Err(Hata::PngBoyutGecersiz { yukseklik: 0, .. }) => {}
            diger => panic!("sifir yukseklik reddedilmeli, alindi: {diger:?}"),
        }
    }

    #[test]
    fn asiri_boyut_reddedilir() {
        match Raster::yeni_sifir(crate::sinir::EN_FAZLA_KENAR + 1, 1) {
            Err(Hata::PngBoyutGecersiz { sebep, .. }) => {
                if !sebep.contains("kenar siniri") {
                    panic!("kenar siniri belirtilmeli: {sebep}");
                }
            }
            diger => panic!("asiri boyut reddedilmeli, alindi: {diger:?}"),
        }
        // Kenar siniri icinde ama piksel siniri asan kombinasyon.
        match Raster::yeni_sifir(99_999, 99_999) {
            Err(Hata::PngBoyutGecersiz { sebep, .. }) => {
                if !sebep.contains("piksel siniri") {
                    panic!("piksel siniri belirtilmeli: {sebep}");
                }
            }
            diger => panic!("piksel siniri reddedilmeli, alindi: {diger:?}"),
        }
    }

    #[test]
    fn satir_okuyucu_sirayla_verir_ve_soner() {
        let mut r = ornek(2, 3);
        r.piksel_ata(1, 0, [1, 1, 1, 1]);
        r.piksel_ata(1, 1, [2, 2, 2, 2]);
        r.piksel_ata(1, 2, [3, 3, 3, 3]);
        let mut okuyucu = satir_okuyucu(&r);
        for beklenen in 1..=3u8 {
            let satir = ac(okuyucu.sonraki());
            match satir {
                Some(s) if s[4] == beklenen => {}
                diger => panic!("satir {beklenen} beklendi, alindi: {diger:?}"),
            }
        }
        match ac(okuyucu.sonraki()) {
            None => {}
            diger => panic!("son satir sonrasi None bekleniyordu: {diger:?}"),
        }
    }

    #[test]
    fn satir_okuyucu_bilgileri_dogru() {
        let r = ornek(5, 7);
        let okuyucu = satir_okuyucu(&r);
        if okuyucu.satir_sayisi() != 7 {
            panic!("satir sayisi 7 olmali");
        }
        if okuyucu.satir_uzunlugu() != 5 * KANAL {
            panic!("satir uzunlugu 20 olmali");
        }
    }

    #[test]
    fn hedefe_sikistirma_boyutu_kucultur() {
        let r = ornek(100, 80);
        let kucuk = ac(r.hedefe_sikistir(50, 40));
        if kucuk.genislik > 50 || kucuk.yukseklik > 40 {
            panic!("hedef asildi: {}x{}", kucuk.genislik, kucuk.yukseklik);
        }
        if kucuk.genislik == 0 || kucuk.yukseklik == 0 {
            panic!("sifir boyut olmamali");
        }
    }

    #[test]
    fn piksel_sayisi_tasma_verir() {
        if piksel_sayisi(u32::MAX, u32::MAX) != 0 {
            panic!("tasma durumunda 0 donmeli");
        }
        if piksel_sayisi(2, 3) != 24 {
            panic!("2x3x4 = 24 olmali");
        }
    }

    #[test]
    fn satir_ata_govdeyi_doldurur() {
        let mut r = ornek(1, 1);
        r.satir_ata(0, &[9; 4]);
        if r.piksel(0, 0) != [9, 9, 9, 9] {
            panic!("yazilan satir okunmadi");
        }
    }
}
