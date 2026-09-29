//! Yeniden boyutlandırma: kutu/alan, en yakın, bilineer ve bicubic örnekleme.
//!
//! ## Sabit bellek modeli
//!
//! Çıktı görüntüsü bellekte tutulur (kaçınılmazdır: hedef boyut araması her
//! denemede tam bir çıktı üretir), **kaynak** görüntünün tamamı ise asla
//! tutulmaz. Kaynak satırlar bir "halka" tamponunda tutulur:
//!
//! - **en yakın**: 1 kaynak satırı,
//! - **bilineer**: en fazla 2 kaynak satırı,
//! - **bicubic**: en fazla 4 kaynak satırı (`BICUBIC_YARICAP = 2`),
//! - **kutu (alan ortalaması)**: `ceil(kaynak_yukseklik / hedef_yukseklik)`
//!   satır; yani `O(kaynak_genislik * kucultme_orani)` bayt.
//!
//! Dolayısıyla bellek `O(cikti_alani + kaynak_genislik * kucultme_orani)` ve
//! kaynak görüntünün tamamı hiçbir zaman belleğe alınmaz.
//!
//! Örnekleme konumu tam sayı aritmetiğiyle hesaplanır
//! (`(2*hedef + 1) * kaynak / (2 * hedef)`); kayan nokta tabanlı birikme
//! olmadığı için sonuç her çalıştırmada bit düzeyinde aynıdır.

use std::collections::VecDeque;

use crate::gorsel::{Raster, SatirOkuyucu, KANAL};
use crate::hata::Hata;
use crate::sinir::bicubic_agirlik;

/// Yeniden boyutlandırmada kullanılacak örnekleme yöntemi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filtre {
    /// Alan (kutu) ortalaması. Küçültmede en yüksek kaliteyi verir.
    #[default]
    Kutu,
    /// En yakın komşu. En hızlı, en kalitesiz.
    EnYakin,
    /// Bilineer enterpolasyon.
    Bilinear,
    /// Bicubic (Catmull-Rom, `a = -0.5`) enterpolasyon.
    Bicubic,
}

impl Filtre {
    /// Yöntemin CLI karşılığı olan adı.
    #[must_use]
    pub fn ad(self) -> &'static str {
        match self {
            Filtre::Kutu => "kutu",
            Filtre::EnYakin => "en-yakin",
            Filtre::Bilinear => "bilinear",
            Filtre::Bicubic => "bicubic",
        }
    }

    /// Verilen addan yöntemi seçer (CLI `--filtre` çözümlemesi).
    ///
    /// # Hatalar
    ///
    /// Bilinmeyen bir ad verilirse `Hata::AyarGecersiz` döner.
    pub fn adindan(ad: &str) -> Result<Self, Hata> {
        match ad {
            "kutu" | "box" | "area" => Ok(Filtre::Kutu),
            "en-yakin" | "nearest" | "nokta" => Ok(Filtre::EnYakin),
            "bilinear" | "lineer" => Ok(Filtre::Bilinear),
            "bicubic" | "kubik" => Ok(Filtre::Bicubic),
            diger => Err(Hata::AyarGecersiz {
                ad: "--filtre".to_string(),
                deger: diger.to_string(),
            }),
        }
    }

    /// Bir örnekleme satırı için gerekli en fazla kaynak satırı sayısı.
    ///
    /// Kutu filtresinde bu sayı küçültme oranına bağlıdır; diğerlerinde sabittir.
    #[must_use]
    pub fn yaricap(self, kaynak_yukseklik: u32, hedef_yukseklik: u32) -> usize {
        match self {
            Filtre::Kutu => {
                let payda = hedef_yukseklik.max(1);
                let oran = kaynak_yukseklik.div_ceil(payda);
                oran.max(1) as usize
            }
            Filtre::Bicubic => (crate::sinir::BICUBIC_YARICAP * 2) as usize,
            Filtre::Bilinear => 2,
            Filtre::EnYakin => 1,
        }
    }

    /// Bir örnekleme satırı için gerekli kaynak satır ağırlıklarını üretir.
    ///
    /// Dönen vektördeki öğeler `(kaynak_indeks, agirlik)` çiftleridir. Ağırlıkların
    /// toplamı 1'dir (bicubic'te ringing nedeniyle negatif ağırlıklar olabilir).
    /// İndeksler kaynak sınırının dışında olabilir; çağıran taraf
    /// `kaynak_uzunluk - 1` ile kıstırır.
    #[must_use]
    pub fn agirliklar(
        self,
        hedef_indeks: u32,
        kaynak_uzunluk: u32,
        hedef_uzunluk: u32,
    ) -> Vec<(usize, f64)> {
        let n = u64::from(kaynak_uzunluk);
        let d = u64::from(hedef_uzunluk);
        let i = u64::from(hedef_indeks);
        match self {
            Filtre::EnYakin => {
                // Cikti pikselinin merkezinin karsilik geldigi kaynak pikseli.
                let sayac = (2 * i + 1) * n;
                let payda = 2 * d;
                vec![((sayac / payda) as usize, 1.0)]
            }
            Filtre::Kutu => {
                let bas = i * n;
                let son = bas + n;
                let ilk = bas / d;
                let son_kaynak = (son - 1) / d;
                (ilk..=son_kaynak)
                    .map(|j| {
                        let ust = j * d;
                        let alt = (j + 1) * d;
                        let kesisim = son.min(alt) - bas.max(ust);
                        (j as usize, kesisim as f64 / n as f64)
                    })
                    .collect()
            }
            Filtre::Bilinear => {
                let sayac = (2 * i + 1) * n;
                let payda = 2 * d;
                let taban = sayac / payda;
                let kalan = (sayac % payda) as f64 / payda as f64;
                vec![
                    (taban.saturating_sub(1) as usize, 1.0 - kalan),
                    (taban as usize, kalan),
                ]
            }
            Filtre::Bicubic => {
                let sayac = (2 * i + 1) * n;
                let payda = 2 * d;
                let taban = (sayac / payda) as i64;
                let t = (sayac % payda) as f64 / payda as f64;
                let w = bicubic_agirlik(t);
                vec![
                    ((taban - 1).max(0) as usize, w[0]),
                    (taban as usize, w[1]),
                    ((taban + 1) as usize, w[2]),
                    ((taban + 2) as usize, w[3]),
                ]
            }
        }
    }
}

/// Kaynak satırlarını sıralı okuyan, "halka" biçiminde önbellek.
struct SatirHalkasi {
    satirlar: VecDeque<(usize, Vec<u8>)>,
    sonraki_indeks: usize,
    tukendi: bool,
}

impl SatirHalkasi {
    /// Yeni, boş bir halka oluşturur.
    fn yeni() -> Self {
        Self {
            satirlar: VecDeque::new(),
            sonraki_indeks: 0,
            tukendi: false,
        }
    }

    /// `alt` indeksinden küçük satırları atar (artık gerekmeyecekler).
    fn temizle(&mut self, alt: usize) {
        while self.satirlar.front().is_some_and(|(i, _)| *i < alt) {
            self.satirlar.pop_front();
        }
    }

    /// Halkanın son satır indeksi `hedef`'ten küçükse kaynaktan okumaya devam eder.
    ///
    /// Kaynak **sıralı** okunduğu ve istenen indeksler azalan olamayacağı için
    /// bu metot yalnızca ileriye doğru okuma yapar.
    fn temin_et(&mut self, hedef: usize, kaynak: &mut dyn SatirOkuyucu) -> Result<(), Hata> {
        while self.sonraki_indeks <= hedef && !self.tukendi {
            match kaynak.sonraki() {
                Ok(Some(satir)) => {
                    let indeks = self.sonraki_indeks;
                    self.sonraki_indeks += 1;
                    self.satirlar.push_back((indeks, satir));
                }
                Ok(None) => self.tukendi = true,
                Err(hata) => return Err(hata),
            }
        }
        Ok(())
    }

    /// `indeks`li satırı dondurür; halkada yoksa `None`.
    fn satir(&self, indeks: usize) -> Option<&[u8]> {
        self.satirlar
            .iter()
            .find(|(i, _)| *i == indeks)
            .map(|(_, v)| v.as_slice())
    }
}

/// Bir `Raster`i yeniden boyutlandırır (en yaygın giriş noktası).
///
/// # Hatalar
///
/// Boyut sıfır veya sınırları aşıyorsa hata döner.
pub fn govde_yeniden_boyutlandir(
    govde: &Raster,
    hedef_genislik: u32,
    hedef_yukseklik: u32,
    filtre: Filtre,
) -> Result<Raster, Hata> {
    let mut okuyucu = crate::gorsel::satir_okuyucu(govde);
    yeniden_boyutlandir(
        &mut okuyucu,
        govde.genislik,
        govde.yukseklik,
        hedef_genislik,
        hedef_yukseklik,
        filtre,
    )
}

/// Sıralı bir satır kaynağından yeniden boyutlandırma yapar.
///
/// Kaynak görüntü belleğe alınmaz; satırlar [`SatirOkuyucu::sonraki`] ile
/// yalnızca gerektiği kadar okunur.
///
/// # Hatalar
///
/// Boyut sıfır/sınır dışıysa, kaynak beklenenden az satır verirse veya
/// okuma sırasında hata oluşursa hata döner.
pub fn yeniden_boyutlandir(
    kaynak: &mut dyn SatirOkuyucu,
    kaynak_genislik: u32,
    kaynak_yukseklik: u32,
    hedef_genislik: u32,
    hedef_yukseklik: u32,
    filtre: Filtre,
) -> Result<Raster, Hata> {
    if hedef_genislik == 0 || hedef_yukseklik == 0 {
        return Err(Hata::BoyutGecersiz {
            ayrinti: format!("hedef boyut {hedef_genislik}x{hedef_yukseklik} sifir olamaz"),
        });
    }
    if kaynak_genislik == 0 || kaynak_yukseklik == 0 {
        return Err(Hata::BoyutGecersiz {
            ayrinti: format!("kaynak boyut {kaynak_genislik}x{kaynak_yukseklik} sifir olamaz"),
        });
    }
    crate::gorsel::boyut_dogrula(kaynak_genislik, u64::from(kaynak_yukseklik))?;
    crate::gorsel::boyut_dogrula(hedef_genislik, u64::from(hedef_yukseklik))?;

    let cikti_satir_uzunluk = hedef_genislik as usize * KANAL;
    let mut halka = SatirHalkasi::yeni();
    let mut toplam = vec![0f64; cikti_satir_uzunluk];
    let mut ham_satir = vec![0u8; cikti_satir_uzunluk];
    let max_y = kaynak_yukseklik as usize - 1;
    let max_x = kaynak_genislik as usize - 1;
    let mut cikti = Raster::yeni_sifir(hedef_genislik, hedef_yukseklik)?;

    // Yatay agirliklar her cikti sutunu icin ayni oldugundan bir kez hesaplanir.
    let yatay: Vec<Vec<(usize, f64)>> = (0..hedef_genislik)
        .map(|ox| filtre.agirliklar(ox, kaynak_genislik, hedef_genislik))
        .collect();

    for oy in 0..hedef_yukseklik {
        let dikey = filtre.agirliklar(oy, kaynak_yukseklik, hedef_yukseklik);
        let en_dusuk = dikey.iter().map(|(i, _)| *i).min().unwrap_or(0).min(max_y);
        let en_yuksek = dikey.iter().map(|(i, _)| *i).max().unwrap_or(0).min(max_y);
        halka.temizle(en_dusuk);
        halka.temin_et(en_yuksek, kaynak)?;

        toplam.iter_mut().for_each(|v| *v = 0.0);
        for (sy, wy) in &dikey {
            let satir = halka.satir((*sy).min(max_y)).ok_or(Hata::BoyutGecersiz {
                ayrinti: format!("kaynak {sy}. satir okunamadi"),
            })?;
            for (ox, agirliklar) in yatay.iter().enumerate() {
                let hedef_ofset = ox * KANAL;
                for (sx, wx) in agirliklar {
                    let kaynak_ofset = (*sx).min(max_x) * KANAL;
                    let w = wy * wx;
                    for k in 0..KANAL {
                        toplam[hedef_ofset + k] += f64::from(satir[kaynak_ofset + k]) * w;
                    }
                }
            }
        }
        for (hedef, deger) in ham_satir.iter_mut().zip(toplam.iter()) {
            *hedef = deger.round().clamp(0.0, 255.0) as u8;
        }
        cikti.satir_ata(oy, &ham_satir);
    }
    Ok(cikti)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_yardimci::ac;

    fn kademeli(g: &mut Raster) {
        for y in 0..g.yukseklik {
            for x in 0..g.genislik {
                let d = ((x + y) * 16) as u8;
                g.piksel_ata(x, y, [d, d, d, 255]);
            }
        }
    }

    fn tek_renk(g: &mut Raster, renk: [u8; KANAL]) {
        for y in 0..g.yukseklik {
            for x in 0..g.genislik {
                g.piksel_ata(x, y, renk);
            }
        }
    }

    fn tumu() -> [Filtre; 4] {
        [
            Filtre::Kutu,
            Filtre::EnYakin,
            Filtre::Bilinear,
            Filtre::Bicubic,
        ]
    }

    #[test]
    fn filtre_adlari_ve_tur_donusumleri() {
        if Filtre::Kutu.ad() != "kutu" || Filtre::Bicubic.ad() != "bicubic" {
            panic!("filtre adlari degismis");
        }
        if ac(Filtre::adindan("bilinear")) != Filtre::Bilinear {
            panic!("bilinear ayristirilmamali");
        }
        if ac(Filtre::adindan("en-yakin")) != Filtre::EnYakin {
            panic!("en-yakin ayristirilmamali");
        }
        if ac(Filtre::adindan("box")) != Filtre::Kutu {
            panic!("box takma adi olmamali");
        }
        match Filtre::adindan("bicubik") {
            Err(Hata::AyarGecersiz { ad, .. }) if ad == "--filtre" => {}
            diger => panic!("gecersiz filtre reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn varsayilan_filtre_kutu() {
        if Filtre::default() != Filtre::Kutu {
            panic!("varsayilan filtre kutu olmali");
        }
    }

    #[test]
    fn yaricap_degerleri_dokumante_edildigi_gibi() {
        if Filtre::Bicubic.yaricap(100, 50) != 4 {
            panic!("bicubic yaricapi 4 olmali");
        }
        if Filtre::Bilinear.yaricap(100, 50) != 2 {
            panic!("bilineer yaricapi 2 olmali");
        }
        if Filtre::EnYakin.yaricap(100, 50) != 1 {
            panic!("en-yakin yaricapi 1 olmali");
        }
        if Filtre::Kutu.yaricap(100, 50) != 2 {
            panic!("kutu yaricapi kucultme orani olmali");
        }
    }

    #[test]
    fn agirliklar_her_zaman_birim_toplamli() {
        for filtre in tumu() {
            for i in 0..7u32 {
                for (n, d) in [(11u32, 7u32), (8, 8), (5, 13), (100, 3)] {
                    let toplam: f64 = filtre.agirliklar(i, n, d).iter().map(|(_, w)| *w).sum();
                    if (toplam - 1.0).abs() > 1e-9 {
                        panic!("{filtre:?} ({n}->{d}) agirlik toplami {toplam}");
                    }
                }
            }
        }
    }

    #[test]
    fn en_yakin_tek_nokta_secer() {
        let a = Filtre::EnYakin.agirliklar(0, 4, 4);
        if a.len() != 1 || a[0].0 != 0 {
            panic!("ilk cikti satiri kaynak 0'a denk gelmeli: {a:?}");
        }
        let b = Filtre::EnYakin.agirliklar(1, 4, 4);
        if b.len() != 1 || b[0].0 != 1 {
            panic!("ikinci cikti satiri kaynak 1'e denk gelmeli: {b:?}");
        }
    }

    #[test]
    fn bilineer_iki_nokta_kullanir() {
        let a = Filtre::Bilinear.agirliklar(0, 8, 8);
        if a.len() != 2 || a[0].0 != 0 {
            panic!("bilineer iki komsu kullanmali: {a:?}");
        }
    }

    #[test]
    fn bicubic_dort_nokta_kullanir() {
        let a = Filtre::Bicubic.agirliklar(0, 8, 8);
        if a.len() != 4 {
            panic!("bicubic dort komsu kullanmali: {a:?}");
        }
    }

    #[test]
    fn kutu_kucultmede_cok_nokta_toplar() {
        let a = Filtre::Kutu.agirliklar(0, 8, 1);
        if a.len() != 8 {
            panic!("8->1 kucultmede 8 satir toplanmali, {} bulundu", a.len());
        }
        let toplam: f64 = a.iter().map(|(_, w)| *w).sum();
        if (toplam - 1.0).abs() > 1e-9 {
            panic!("kutu agirliklari toplami 1 degil: {toplam}");
        }
    }

    #[test]
    fn kutu_buyutmede_tek_nokta_verir() {
        let a = Filtre::Kutu.agirliklar(0, 1, 4);
        if a.len() != 1 || (a[0].1 - 1.0).abs() > 1e-12 {
            panic!("buyutmede kutu tek nokta vermeli: {a:?}");
        }
    }

    #[test]
    fn sifir_boyut_reddedilir() {
        let g = ac(Raster::yeni_sifir(2, 2));
        match govde_yeniden_boyutlandir(&g, 0, 5, Filtre::Bilinear) {
            Err(Hata::BoyutGecersiz { ayrinti }) => {
                if !ayrinti.contains("sifir olamaz") {
                    panic!("ayrinti sifir olmali: {ayrinti}");
                }
            }
            diger => panic!("sifir genislik reddedilmeli: {diger:?}"),
        }
    }

    #[test]
    fn asiri_hedef_boyut_reddedilir() {
        let g = ac(Raster::yeni_sifir(2, 2));
        if govde_yeniden_boyutlandir(&g, crate::sinir::EN_FAZLA_KENAR + 1, 2, Filtre::Kutu).is_ok()
        {
            panic!("sinir asimi reddedilmeli");
        }
    }

    #[test]
    fn tek_renk_tum_filtrelerde_korunur() {
        let mut g = ac(Raster::yeni_sifir(16, 16));
        tek_renk(&mut g, [10, 200, 30, 128]);
        for filtre in tumu() {
            let k = ac(govde_yeniden_boyutlandir(&g, 4, 4, filtre));
            if k.piksel(2, 2) != [10, 200, 30, 128] {
                panic!("{filtre:?} tek renkli gorseli bozdu: {:?}", k.piksel(2, 2));
            }
        }
    }

    #[test]
    fn kutu_ortalama_degerini_verir() {
        // 2x2 siyah-beyaz dama kumesi -> 1x1 kutu ortalamasi 127.5.
        let mut g = ac(Raster::yeni_sifir(2, 2));
        g.piksel_ata(0, 0, [0, 0, 0, 255]);
        g.piksel_ata(1, 0, [255, 255, 255, 255]);
        g.piksel_ata(0, 1, [255, 255, 255, 255]);
        g.piksel_ata(1, 1, [0, 0, 0, 255]);
        let k = ac(govde_yeniden_boyutlandir(&g, 1, 1, Filtre::Kutu));
        if k.piksel(0, 0)[0] != 128 {
            panic!("kutu ortalamasi 128 olmali, {:?}", k.piksel(0, 0));
        }
    }

    #[test]
    fn kutu_kismi_kapsama_agirligini_hesaplar() {
        // 4 -> 2: her kaynak satirinin yarisi bir cikti satirina girer.
        let mut g = ac(Raster::yeni_sifir(1, 4));
        g.piksel_ata(0, 0, [0, 0, 0, 255]);
        g.piksel_ata(0, 1, [100, 0, 0, 255]);
        g.piksel_ata(0, 2, [200, 0, 0, 255]);
        g.piksel_ata(0, 3, [255, 0, 0, 255]);
        let k = ac(govde_yeniden_boyutlandir(&g, 1, 2, Filtre::Kutu));
        if k.piksel(0, 0)[0] != 50 {
            panic!("ilk satir ortalamasi 50 olmali, {}", k.piksel(0, 0)[0]);
        }
        if k.piksel(0, 1)[0] != 228 {
            panic!("ikinci satir ortalamasi 228 olmali, {}", k.piksel(0, 1)[0]);
        }
    }

    #[test]
    fn butun_filtreler_hedef_boyutu_verir() {
        let mut g = ac(Raster::yeni_sifir(10, 8));
        kademeli(&mut g);
        for filtre in tumu() {
            let k = ac(govde_yeniden_boyutlandir(&g, 5, 4, filtre));
            if k.genislik != 5 || k.yukseklik != 4 {
                panic!("{filtre:?} hedefi vermedi: {}x{}", k.genislik, k.yukseklik);
            }
            if k.piksel.len() != 5 * 4 * KANAL {
                panic!("{filtre:?} piksel sayisi yanlis");
            }
        }
    }

    #[test]
    fn buyutme_ve_alfa_korunumu() {
        let mut g = ac(Raster::yeni_sifir(2, 2));
        kademeli(&mut g);
        let k = ac(govde_yeniden_boyutlandir(&g, 16, 16, Filtre::Bicubic));
        if k.genislik != 16 || k.yukseklik != 16 {
            panic!("buyutme calismadi");
        }
        let mut a = ac(Raster::yeni_sifir(4, 4));
        tek_renk(&mut a, [100, 100, 100, 64]);
        let k2 = ac(govde_yeniden_boyutlandir(&a, 2, 2, Filtre::Bilinear));
        if k2.piksel(0, 0)[3] != 64 {
            panic!("alfa korunmadi: {:?}", k2.piksel(0, 0));
        }
    }

    #[test]
    fn ayni_girdi_ayni_sonuc_verir() {
        let mut g = ac(Raster::yeni_sifir(7, 5));
        kademeli(&mut g);
        let a = ac(govde_yeniden_boyutlandir(&g, 3, 3, Filtre::Bicubic));
        let b = ac(govde_yeniden_boyutlandir(&g, 3, 3, Filtre::Bicubic));
        if a != b {
            panic!("yeniden boyutlandirma tekrarlanabilir olmali");
        }
    }

    #[test]
    fn satir_strasi_kaynaktan_okunur() {
        struct TekSatirlik {
            ic: Vec<Vec<u8>>,
            sira: usize,
        }
        impl SatirOkuyucu for TekSatirlik {
            fn sonraki(&mut self) -> Result<Option<Vec<u8>>, Hata> {
                if self.sira >= self.ic.len() {
                    return Ok(None);
                }
                let s = self.ic[self.sira].clone();
                self.sira += 1;
                Ok(Some(s))
            }
            fn satir_sayisi(&self) -> u64 {
                self.ic.len() as u64
            }
            fn satir_uzunlugu(&self) -> usize {
                self.ic.first().map_or(0, Vec::len)
            }
        }
        let mut g = ac(Raster::yeni_sifir(3, 4));
        kademeli(&mut g);
        let ic: Vec<Vec<u8>> = (0..g.yukseklik).map(|y| g.satir(y)).collect();
        let mut kaynak = TekSatirlik { ic, sira: 0 };
        let k = ac(yeniden_boyutlandir(
            &mut kaynak,
            3,
            4,
            6,
            2,
            Filtre::Bilinear,
        ));
        if k.genislik != 6 || k.yukseklik != 2 {
            panic!("beklenmedik boyut");
        }
        if kaynak.sira > 4 {
            panic!("kaynaktan fazla satir okundu: {}", kaynak.sira);
        }
    }

    #[test]
    fn yetersiz_kaynak_satiri_hata_dondurur() {
        struct BosKaynak;
        impl SatirOkuyucu for BosKaynak {
            fn sonraki(&mut self) -> Result<Option<Vec<u8>>, Hata> {
                Ok(None)
            }
            fn satir_sayisi(&self) -> u64 {
                1
            }
            fn satir_uzunlugu(&self) -> usize {
                8
            }
        }
        let mut kaynak = BosKaynak;
        match yeniden_boyutlandir(&mut kaynak, 2, 50, 2, 50, Filtre::Bilinear) {
            Err(Hata::BoyutGecersiz { ayrinti }) => {
                if !ayrinti.contains("okunamadi") {
                    panic!("ayrinti satir eksikligini belirtmeli: {ayrinti}");
                }
            }
            diger => panic!("yetersiz kaynak hatasi bekleniyordu: {diger:?}"),
        }
    }

    #[test]
    fn halka_yardimcilari_dogru_calisir() {
        let mut h = SatirHalkasi::yeni();
        h.satirlar.push_back((3, vec![1, 2, 3, 4]));
        if h.satir(3) != Some([1u8, 2, 3, 4].as_slice()) {
            panic!("satir bulunamadi");
        }
        if h.satir(4).is_some() {
            panic!("olmayan satir donmemeli");
        }
        h.temizle(4);
        if h.satir(3).is_some() {
            panic!("temizleme satiri atmali");
        }
        if h.sonraki_indeks != 0 || h.tukendi {
            panic!("yeni halka sifirdan baslamali");
        }
    }
}
