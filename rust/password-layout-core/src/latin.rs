//! Whether a keyboard layout types Latin letters. Measured rather than
//! guessed: every layout and variant of xkeyboard-config 2.48 was compiled
//! with libxkbcommon and counts as Latin if its base level types at least 20
//! of the 26 letters a-z (tools/xkb-latin.c; the result is
//! tests/xkb-latin.tsv, which the tests check this against). Esperanto or
//! Azerbaijani, a few letters short, stay Latin; a script with a stray Latin
//! letter does not.

/// C++ `isLatin` in src/compositor.cpp.
pub fn is_latin(layout: &str, variant: &str) -> bool {
    // Layouts whose base form types no Latin.
    const NON_LATIN: &str = concat!(
        " af am ara bd bg brai bt by eg et ge gn gr il in iq ir kg kh kz la lk ma mk mm mn ",
        "mv my np pk rs ru sy th tj tz ua uz ",
    );
    // Variants that differ from their layout, beyond those named *latin*.
    const LATIN_VARIANTS: &str = concat!(
        " in(eng) in(mara) iq(ku) iq(ku_alt) iq(ku_f) ir(ku) ir(ku_alt) ir(ku_f) lk(us) ",
        "ma(french) ma(rif) mm(mara) ru(ruchey_en) sy(ku) sy(ku_alt) sy(ku_f) ua(crh) ",
        "ua(crh_alt) ua(crh_f) ",
    );
    const NON_LATIN_VARIANTS: &str = concat!(
        " al(veqilharxhi) az(cyrillic) br(rus) ca(ike) cn(mon_manchu_galik) cn(mon_todo_galik) ",
        "cn(mon_trad) cn(mon_trad_galik) cn(mon_trad_manchu) cn(mon_trad_todo) ",
        "cn(mon_trad_xibe) cn(tib) cn(tib_asciinum) cn(ug) cz(rus) cz(ucw) de(ru) dz(ar) ",
        "dz(ber) fr(geo) id(javanese) id(melayu-phonetic) id(melayu-phoneticx) ",
        "id(pegon-phonetic) ie(ogam) it(geo) jp(kana) jp(mac) lv(modern-cyr) me(cyrillic) ",
        "me(cyrillicalternatequotes) me(cyrillicyz) ph(capewell-dvorak-bay) ",
        "ph(capewell-qwerf2k6-bay) ph(colemak-bay) ph(dvorak-bay) ph(qwerty-bay) ",
        "pl(ru_phonetic_dvorak) se(rus) se(swl) us(chr) us(rus) ",
    );

    fn listed(list: &str, name: &str) -> bool {
        list.contains(format!(" {name} ").as_str())
    }

    if !variant.is_empty() {
        let named = format!("{layout}({variant})");
        if listed(NON_LATIN_VARIANTS, &named) {
            return false;
        }
        if listed(LATIN_VARIANTS, &named) || variant.contains("latin") {
            return true;
        }
    }
    !listed(NON_LATIN, layout)
}

/// C++ `latinLayouts` in src/compositor.cpp.
pub fn latin_layouts(layouts: &str, variants: &str) -> Vec<bool> {
    let names = split_commas(layouts);
    let kinds = split_commas(variants);
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            !name.is_empty() && is_latin(name, kinds.get(index).copied().unwrap_or(""))
        })
        .collect()
}

// C++ `splitCommas`: "" gives one empty name.
fn split_commas(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    loop {
        match text[start..].find(',') {
            None => {
                parts.push(&text[start..]);
                return parts;
            }
            Some(comma) => {
                parts.push(&text[start..start + comma]);
                start += comma + 1;
            }
        }
    }
}
