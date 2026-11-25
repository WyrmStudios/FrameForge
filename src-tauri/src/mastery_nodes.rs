//! Amounts come from https://wiki.warframe.com/w/Module:Missions/data.
//! Run `python3 src-tauri/src/update_mastery_nodes.py` to refresh them.
//! Zero amounts stay in the table so a refresh can detect changed rewards.
//!
//! TODO: Höllvania (SolNode850 to SolNode858) is not on the checklist yet.
//! Add its nodes once they are confirmed to award mastery.
//! TODO: no removed node is listed. A node dropped from the chart keeps its
//! row here under `Unobtainable::RemovedNode` once one is identified.

pub(crate) const JUNCTION_MASTERY: u32 = 1_000;

#[derive(Debug)]
pub(crate) struct Planet {
    pub(crate) name: &'static str,
    /// The junction the planet is entered through. Nothing on the planet is
    /// reachable before it is cleared, short of a squad invite onto the node.
    pub(crate) gate: Option<&'static str>,
    /// Each entry is the node key and its display name. A junction is listed
    /// under the planet it sits on, so the Sedna Junction is on Pluto although
    /// its key says Eris.
    pub(crate) junctions: &'static [(&'static str, &'static str)],
    pub(crate) nodes: &'static [(&'static str, &'static str, u32)],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub(crate) key: &'static str,
    pub(crate) planet: &'static Planet,
    pub(crate) name: &'static str,
    pub(crate) junction: bool,
    pub(crate) amount: u32,
}

pub(crate) fn all() -> impl Iterator<Item = Node> {
    PLANETS.iter().flat_map(|planet| {
        let junctions = planet.junctions.iter().map(move |&(key, name)| Node { key, planet, name, junction: true, amount: JUNCTION_MASTERY });
        let nodes = planet.nodes.iter().map(move |&(key, name, amount)| Node { key, planet, name, junction: false, amount });
        junctions.chain(nodes)
    })
}

pub(crate) const PLANETS: &[Planet] = &[
    Planet { name: "Mercury", gate: Some("VenusToMercuryJunction"), junctions: &[], nodes: &[
        ("SolNode94", "Apollodorus", 0),
        ("SolNode223", "Boethius", 3),
        ("SolNode119", "Caloris", 3),
        ("SolNode12", "Elion", 3),
        ("SolNode130", "Lares", 3),
        ("SolNode103", "M Prime", 3),
        ("SolNode224", "Odin", 3),
        ("SolNode226", "Pantheon", 3),
        ("SolNode225", "Suisei", 3),
        ("SolNode28", "Terminus", 0),
        ("SolNode108", "Tolstoj", 25),
    ] },
    Planet { name: "Venus", gate: Some("EarthToVenusJunction"), junctions: &[
        ("VenusToMercuryJunction", "Mercury Junction"),
    ], nodes: &[
        ("SolNode2", "Aphrodite", 18),
        ("SolNode23", "Cytherean", 18),
        ("SolNode128", "E Gate", 18),
        ("SolNode239", "Follie's Hunt", 50),
        ("SolNode104", "Fossa", 41),
        ("SolNode61", "Ishtar", 24),
        ("SolNode101", "Kiliken", 18),
        ("SolNode109", "Linea", 18),
        ("ClanNode1", "Malva", 0),
        ("SolNode902", "Montes", 18),
        ("SolNode129", "Orb Vallis", 24),
        ("ClanNode0", "Romula", 0),
        ("SolNode22", "Tessera", 18),
        ("SolNode66", "Unda", 18),
        ("SolNode123", "V Prime", 18),
        ("SolNode107", "Venera", 18),
    ] },
    Planet { name: "Earth", gate: None, junctions: &[
        ("EarthToMarsJunction", "Mars Junction"),
        ("EarthToVenusJunction", "Venus Junction"),
    ], nodes: &[
        ("SolNode79", "Cambria", 24),
        ("SolNode75", "Cervantes", 24),
        ("ClanNode2", "Coba", 0),
        ("SolNode27", "E Prime", 24),
        ("SolNode903", "Erpo", 24),
        ("SolNode59", "Eurasia", 24),
        ("SolNode39", "Everest", 24),
        ("SolNode85", "Gaia", 20),
        ("SolNode26", "Lith", 24),
        ("SolNode63", "Mantle", 24),
        ("SolNode89", "Mariana", 24),
        ("SolNode24", "Oro", 24),
        ("SolNode15", "Pacific", 24),
        ("SolNode228", "Plains of Eidolon", 24),
        ("ClanNode3", "Tikal", 0),
    ] },
    Planet { name: "Lua", gate: None, junctions: &[], nodes: &[
        ("SolNode308", "Apollo", 0),
        ("SolNode310", "Circulus", 0),
        ("SolNode304", "Copernicus", 0),
        ("SolNode301", "Grimaldi", 0),
        ("SolNode306", "Pavlov", 0),
        ("SolNode300", "Plato", 0),
        ("SolNode305", "Stöfler", 0),
        ("SolNode302", "Tycho", 0),
        ("SolNode309", "Yuvarium", 0),
        ("SolNode307", "Zeipel", 0),
    ] },
    Planet { name: "Mars", gate: Some("EarthToMarsJunction"), junctions: &[
        ("MarsToCeresJunction", "Ceres Junction"),
        ("MarsToPhobosJunction", "Phobos Junction"),
    ], nodes: &[
        ("SolNode106", "Alator", 51),
        ("SolNode45", "Ara", 51),
        ("SolNode113", "Ares", 51),
        ("SolNode41", "Arval", 51),
        ("SolNode16", "Augustus", 51),
        ("SolNode65", "Gradivus", 45),
        ("SolNode58", "Hellas", 51),
        ("ClanNode8", "Kadesh", 0),
        ("SolNode36", "Martialis", 51),
        ("SolNode30", "Olympus", 51),
        ("SolNode46", "Spear", 51),
        ("SolNode904", "Syrtis", 51),
        ("SolNode11", "Tharsis", 51),
        ("SolNode450", "Tyana Pass", 18),
        ("SolNode14", "Ultor", 51),
        ("SolNode68", "Vallis", 51),
        ("ClanNode9", "Wahiba", 0),
        ("SolNode99", "War", 51),
    ] },
    Planet { name: "Deimos", gate: None, junctions: &[], nodes: &[
        ("SolNode721", "Armatus", 0),
        ("SolNode229", "Cambion Drift", 0),
        ("SolNode718", "Cambire", 0),
        ("SolNode709", "Dirus", 0),
        ("SolNode715", "Effervo", 0),
        ("SolNode713", "Exequias", 0),
        ("SolNode710", "Formido", 0),
        ("SolNode706", "Horend", 0),
        ("SolNode707", "Hyf", 0),
        ("SolNode712", "Magnacidium", 0),
        ("SolNode719", "Munio", 0),
        ("SolNode716", "Nex", 0),
        ("SolNode717", "Persto", 0),
        ("SolNode708", "Phlegyas", 0),
        ("SolNode711", "Terrorem", 0),
        ("SolNode720", "Testudo", 0),
    ] },
    Planet { name: "Phobos", gate: Some("MarsToPhobosJunction"), junctions: &[], nodes: &[
        ("SettlementNode11", "Gulliver", 157),
        ("SettlementNode20", "Iliad", 100),
        ("SettlementNode10", "Kepler", 157),
        ("ClanNode10", "Memphis", 0),
        ("SettlementNode12", "Monolith", 157),
        ("SettlementNode1", "Roche", 157),
        ("SettlementNode15", "Sharpless", 157),
        ("SettlementNode14", "Shklovsky", 157),
        ("SettlementNode2", "Skyresh", 157),
        ("SettlementNode3", "Stickney", 157),
        ("ClanNode11", "Zeugma", 0),
    ] },
    Planet { name: "Ceres", gate: Some("MarsToCeresJunction"), junctions: &[
        ("CeresToJupiterJunction", "Jupiter Junction"),
    ], nodes: &[
        ("SolNode132", "Bode", 163),
        ("SolNode149", "Casta", 163),
        ("SolNode147", "Cinxia", 163),
        ("SolNode146", "Draco", 163),
        ("SolNode144", "Exta", 163),
        ("ClanNode23", "Gabii", 0),
        ("SolNode141", "Ker", 163),
        ("SolNode140", "Kiste", 163),
        ("SolNode139", "Lex", 163),
        ("SolNode138", "Ludi", 163),
        ("SolNode137", "Nuovo", 163),
        ("SolNode131", "Pallas", 163),
        ("ClanNode22", "Seimeni", 0),
        ("SolNode135", "Thon", 163),
    ] },
    Planet { name: "Jupiter", gate: Some("CeresToJupiterJunction"), junctions: &[
        ("JupiterToEuropaJunction", "Europa Junction"),
        ("JupiterToSaturnJunction", "Saturn Junction"),
    ], nodes: &[
        ("SolNode88", "Adrastea", 51),
        ("SolNode97", "Amalthea", 51),
        ("SolNode73", "Ananke", 51),
        ("SolNode25", "Callisto", 51),
        ("ClanNode5", "Cameria", 0),
        ("SolNode74", "Carme", 51),
        ("SolNode121", "Carpo", 51),
        ("SolNode100", "Elara", 51),
        ("SolNode905", "Galilea", 51),
        ("SolNode87", "Ganymede", 51),
        ("SolNode125", "Io", 51),
        ("SolNode126", "Metis", 51),
        ("ClanNode4", "Sinai", 0),
        ("SolNode740", "The Ropalolyst", 55),
        ("SolNode10", "Thebe", 51),
        ("SolNode53", "Themisto", 51),
    ] },
    Planet { name: "Europa", gate: Some("JupiterToEuropaJunction"), junctions: &[], nodes: &[
        ("SolNode203", "Abaddon", 138),
        ("SolNode204", "Armaros", 138),
        ("SolNode205", "Baal", 138),
        ("ClanNode7", "Cholistan", 0),
        ("SolNode220", "Kokabiel", 138),
        ("ClanNode6", "Larzac", 0),
        ("SolNode209", "Morax", 138),
        ("SolNode210", "Naamah", 138),
        ("SolNode217", "Orias", 138),
        ("SolNode211", "Ose", 138),
        ("SolNode212", "Paimon", 138),
        ("SolNode214", "Sorath", 138),
        ("SolNode215", "Valac", 138),
        ("SolNode216", "Valefor", 138),
    ] },
    Planet { name: "Saturn", gate: Some("JupiterToSaturnJunction"), junctions: &[
        ("SaturnToUranusJunction", "Uranus Junction"),
    ], nodes: &[
        ("SolNode31", "Anthe", 55),
        ("SolNode82", "Calypso", 55),
        ("ClanNode12", "Caracol", 0),
        ("SolNode70", "Cassini", 55),
        ("SolNode67", "Dione", 55),
        ("SolNode19", "Enceladus", 49),
        ("SolNode42", "Helene", 55),
        ("SolNode93", "Keeler", 55),
        ("SolNode50", "Numa", 55),
        ("SolNode906", "Pandora", 55),
        ("ClanNode13", "Piscinas", 0),
        ("SolNode18", "Rhea", 55),
        ("SolNode20", "Telesto", 55),
        ("SolNode32", "Tethys", 55),
        ("SolNode96", "Titan", 55),
    ] },
    Planet { name: "Uranus", gate: Some("SaturnToUranusJunction"), junctions: &[
        ("UranusToNeptuneJunction", "Neptune Junction"),
    ], nodes: &[
        ("SolNode33", "Ariel", 69),
        ("ClanNode17", "Assur", 0),
        ("SolNode723", "Brutus", 0),
        ("SolNode907", "Caelus", 69),
        ("SolNode60", "Caliban", 69),
        ("SolNode83", "Cressida", 69),
        ("SolNode98", "Desdemona", 69),
        ("SolNode69", "Ophelia", 69),
        ("SolNode114", "Puck", 44),
        ("SolNode9", "Rosalind", 69),
        ("SolNode122", "Stephano", 69),
        ("SolNode34", "Sycorax", 69),
        ("SolNode105", "Titania", 69),
        ("SolNode64", "Umbriel", 69),
        ("ClanNode16", "Ur", 0),
    ] },
    Planet { name: "Neptune", gate: Some("UranusToNeptuneJunction"), junctions: &[
        ("NeptuneToPlutoJunction", "Pluto Junction"),
    ], nodes: &[
        ("SolNode6", "Despina", 52),
        ("SolNode1", "Galatea", 52),
        ("ClanNode21", "Kelashin", 0),
        ("SolNode118", "Laomedeia", 52),
        ("SolNode49", "Larissa", 52),
        ("SolNode84", "Nereid", 52),
        ("SolNode62", "Neso", 52),
        ("SolNode17", "Proteus", 52),
        ("SolNode127", "Psamathe", 52),
        ("SolNode908", "Salacia", 52),
        ("SolNode57", "Sao", 52),
        ("SolNode78", "Triton", 52),
        ("ClanNode20", "Yursa", 0),
    ] },
    Planet { name: "Pluto", gate: Some("NeptuneToPlutoJunction"), junctions: &[
        ("PlutoToErisJunction", "Eris Junction"),
        ("ErisToSednaJunction", "Sedna Junction"),
    ], nodes: &[
        ("SolNode4", "Acheron", 51),
        ("SolNode43", "Cerberus", 51),
        ("SolNode56", "Cypress", 51),
        ("SolNode51", "Hades", 51),
        ("ClanNode25", "Hieracon", 0),
        ("SolNode76", "Hydra", 51),
        ("SolNode38", "Minthe", 51),
        ("SolNode21", "Narcissus", 51),
        ("SolNode102", "Oceanum", 51),
        ("SolNode72", "Outer Terminus", 51),
        ("SolNode81", "Palus", 51),
        ("SolNode48", "Regna", 51),
        ("ClanNode24", "Sechura", 0),
    ] },
    Planet { name: "Eris", gate: Some("PlutoToErisJunction"), junctions: &[], nodes: &[
        ("ClanNode18", "Akkad", 0),
        ("SolNode153", "Brugia", 279),
        ("SolNode162", "Isos", 279),
        ("SolNode164", "Kala-azar", 279),
        ("SolNode175", "Naeglar", 279),
        ("SolNode166", "Nimus", 279),
        ("SolNode167", "Oestrus", 279),
        ("SolNode171", "Saxis", 279),
        ("SolNode173", "Solium", 279),
        ("SolNode172", "Xini", 279),
        ("ClanNode19", "Zabala", 0),
    ] },
    Planet { name: "Sedna", gate: Some("ErisToSednaJunction"), junctions: &[], nodes: &[
        ("SolNode181", "Adaro", 177),
        ("ClanNode14", "Amarna", 0),
        ("SolNode185", "Berehynia", 50),
        ("SolNode196", "Charybdis", 177),
        ("SolNode195", "Hydron", 177),
        ("SolNode177", "Kappa", 177),
        ("SolNode188", "Kelpie", 177),
        ("SolNode191", "Marid", 177),
        ("SolNode193", "Merrow", 100),
        ("SolNode189", "Naga", 177),
        ("SolNode190", "Nakki", 177),
        ("SolNode184", "Rusalka", 177),
        ("ClanNode15", "Sangeru", 0),
        ("SolNode187", "Selkie", 177),
        ("SolNode183", "Vodyanoi", 177),
        ("SolNode199", "Yam", 177),
    ] },
    Planet { name: "Void", gate: None, junctions: &[], nodes: &[
        ("SolNode405", "Ani", 0),
        ("SolNode410", "Aten", 0),
        ("SolNode408", "Belenus", 0),
        ("SolNode401", "Hepit", 0),
        ("SolNode411", "Marduk", 0),
        ("SolNode412", "Mithra", 0),
        ("SolNode409", "Mot", 0),
        ("SolNode407", "Oxomoco", 0),
        ("SolNode404", "Stribog", 0),
        ("SolNode402", "Taranis", 0),
        ("SolNode400", "Teshub", 0),
        ("SolNode403", "Tiwaz", 0),
        ("SolNode406", "Ukko", 0),
    ] },
    Planet { name: "Kuva Fortress", gate: None, junctions: &[], nodes: &[
        ("SolNode746", "Dakata", 0),
        ("SolNode748", "Garus", 0),
        ("SolNode741", "Koro", 0),
        ("SolNode742", "Nabuk", 0),
        ("SolNode747", "Pago", 0),
        ("SolNode743", "Rotuma", 0),
        ("SolNode745", "Tamu", 0),
        ("SolNode744", "Taveuni", 0),
    ] },
    Planet { name: "Zariman", gate: None, junctions: &[], nodes: &[
        ("SolNode230", "Everview Arc", 0),
        ("SolNode231", "Halako Perimeter", 0),
        ("SolNode233", "Oro Works", 0),
        ("SolNode235", "The Greenway", 0),
        ("SolNode232", "Tuvul Commons", 0),
    ] },
    Planet { name: "Duviri", gate: None, junctions: &[], nodes: &[
        ("SolNode238", "The Circuit", 0),
        ("SolNode236", "The Duviri Experience", 0),
        ("SolNode237", "The Lone Story", 0),
    ] },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_chart_has_thirteen_junctions_and_no_duplicate_key() {
        let nodes: Vec<Node> = all().collect();
        assert_eq!(nodes.len(), 266);
        assert_eq!(nodes.iter().map(|n| n.amount).sum::<u32>(), 27_569);
        assert_eq!(nodes.iter().filter(|n| n.amount > 0).count(), 182);
        assert_eq!(nodes.iter().filter(|n| n.junction).count(), 13);
        let keys: HashSet<&str> = nodes.iter().map(|n| n.key).collect();
        assert_eq!(keys.len(), nodes.len());
        for planet in PLANETS {
            if let Some(gate) = planet.gate {
                assert!(keys.contains(gate), "{}: gate {gate} is a listed junction", planet.name);
            }
        }
        let sedna = nodes.iter().find(|n| n.key == "ErisToSednaJunction").expect("listed");
        assert_eq!((sedna.planet.name, sedna.name, sedna.junction), ("Pluto", "Sedna Junction", true));
    }
}
