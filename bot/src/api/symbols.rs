// symbols.rs — cache of cTrader symbol_id ↔ symbol_name + per-symbol trading metadata.
//
// Populated in two passes:
//   1. `populate()` — runs after `ProtoOASymbolsListReq` lands the list of
//      (id, name) pairs. Enough to resolve ids on WS frames.
//   2. `put_meta()` — runs after `ProtoOASymbolByIdReq` for a subset of
//      symbols (typically the ones the bot is going to subscribe to or
//      trade). Provides minVolume / stepVolume / digits / pipPosition.
//
// Lookups always return a string so serialisation can't fail; unknown
// ids are logged once per id and rendered as "SYMBOL_<id>".

use dashmap::DashMap;
use std::sync::Arc;
use tracing::warn;

/// Per-symbol trading metadata from `ProtoOaSymbol`. Fields are in the
/// broker's raw units — conversion to human units happens at the API
/// boundary (see `dto::SymbolInfo::from_catalog`).
#[derive(Clone, Copy, Debug)]
pub struct SymbolMeta {
    /// Price display precision (e.g. EURUSD = 5, USDJPY = 3).
    pub digits: u32,
    /// Which decimal is the pip (e.g. EURUSD = 4 so pip = 0.0001,
    /// USDJPY = 2 so pip = 0.01).
    pub pip_position: u32,
    /// Smallest order volume the broker will accept, in cTrader "cents"
    /// (= 1/100 of the base asset unit).
    pub min_volume: i64,
    /// Volume tick — order volume must be a multiple of this, in cents.
    pub step_volume: i64,
    /// Largest order volume the broker will accept, in cents.
    pub max_volume: i64,
}

#[derive(Default)]
pub struct SymbolCatalog {
    map: DashMap<i64, String>,
    reverse: DashMap<String, i64>,
    meta: DashMap<i64, SymbolMeta>,
}

impl SymbolCatalog {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Populate the id ↔ name mapping from a `ProtoOaSymbolsListRes`.
    /// Does not touch the metadata store.
    pub fn populate<'a, I>(&self, entries: I) -> usize
    where
        I: IntoIterator<Item = (i64, &'a str)>,
    {
        let mut count = 0usize;
        for (id, name) in entries {
            self.map.insert(id, name.to_string());
            self.reverse.insert(name.to_string(), id);
            count += 1;
        }
        count
    }

    /// Attach trading metadata for a single symbol — called once per
    /// entry in `ProtoOaSymbolByIdRes`. Idempotent: a second call for
    /// the same id overwrites.
    pub fn put_meta(&self, id: i64, meta: SymbolMeta) {
        self.meta.insert(id, meta);
    }

    /// Reverse lookup: symbol name to id. Case-sensitive exact match.
    pub fn id(&self, name: &str) -> Option<i64> {
        self.reverse.get(name).map(|e| *e.value())
    }

    /// Resolve an id to a symbol name. Unknown ids get a stable placeholder
    /// and are logged at WARN so a catalog miss is visible.
    pub fn name(&self, id: i64) -> String {
        if let Some(entry) = self.map.get(&id) {
            return entry.clone();
        }
        let fallback = format!("SYMBOL_{id}");
        warn!(symbol_id = id, "symbol catalog miss; using placeholder");
        fallback
    }

    /// Metadata for a symbol id. `None` if we haven't fetched details
    /// for that id (typical for symbols we neither subscribe nor trade).
    ///
    /// Unused on the REST path today (`entries_with_meta` covers the
    /// listing endpoint) but kept as the natural lookup primitive for
    /// future client-side order validation on the bot.
    #[allow(dead_code)]
    pub fn meta(&self, id: i64) -> Option<SymbolMeta> {
        self.meta.get(&id).map(|e| *e.value())
    }

    /// Iterate `(id, name, meta)` for every symbol that has both a name
    /// AND metadata. Used by `GET /api/symbols` to expose the set of
    /// symbols the UI can sensibly validate against.
    pub fn entries_with_meta(&self) -> Vec<(i64, String, SymbolMeta)> {
        self.meta
            .iter()
            .filter_map(|entry| {
                let id = *entry.key();
                let m = *entry.value();
                let name = self.map.get(&id).map(|n| n.value().clone())?;
                Some((id, name, m))
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    // Required by clippy's `len_without_is_empty` for any type with a
    // public `len()`. Not called today; present to satisfy the lint.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_meta() -> SymbolMeta {
        SymbolMeta {
            digits: 5,
            pip_position: 4,
            min_volume: 100_000,
            step_volume: 10_000,
            max_volume: 10_000_000,
        }
    }

    #[test]
    fn populate_and_lookup() {
        let cat = SymbolCatalog::new();
        cat.populate([(1, "EURUSD"), (2, "GBPUSD")]);
        assert_eq!(cat.name(1), "EURUSD");
        assert_eq!(cat.name(2), "GBPUSD");
        assert_eq!(cat.len(), 2);
    }

    #[test]
    fn missing_lookup_returns_placeholder() {
        let cat = SymbolCatalog::new();
        assert_eq!(cat.name(99), "SYMBOL_99");
    }

    #[test]
    fn reverse_lookup_roundtrips() {
        let cat = SymbolCatalog::new();
        cat.populate([(7, "XAUUSD")]);
        assert_eq!(cat.id("XAUUSD"), Some(7));
        assert!(cat.id("UNKNOWN").is_none());
    }

    #[test]
    fn meta_round_trips() {
        let cat = SymbolCatalog::new();
        cat.populate([(1, "EURUSD")]);
        cat.put_meta(1, fake_meta());
        let m = cat.meta(1).unwrap();
        assert_eq!(m.digits, 5);
        assert_eq!(m.min_volume, 100_000);
    }

    #[test]
    fn meta_missing_returns_none() {
        let cat = SymbolCatalog::new();
        cat.populate([(1, "EURUSD")]);
        assert!(cat.meta(1).is_none(), "no meta attached yet");
    }

    #[test]
    fn entries_with_meta_skips_symbols_without_details() {
        let cat = SymbolCatalog::new();
        cat.populate([(1, "EURUSD"), (2, "GBPUSD"), (3, "XAUUSD")]);
        cat.put_meta(1, fake_meta());
        cat.put_meta(3, fake_meta());
        let mut names: Vec<String> =
            cat.entries_with_meta().into_iter().map(|(_, n, _)| n).collect();
        names.sort();
        assert_eq!(names, vec!["EURUSD", "XAUUSD"]);
    }
}
