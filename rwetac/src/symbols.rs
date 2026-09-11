//! Symbol table used during typechecking and code generation.
//!
//! The [`SymbolTable`] maps identifier names to their [`Type`] and [`Attr`]
//! (whether the symbol is a local variable, global, function, or record definition).
//! It is populated during typechecking and consumed by WTAC generation and the emitter.

use std::collections::HashMap;

use crate::types::Type;

/// Describes what kind of symbol an identifier refers to.
#[derive(Debug, Clone)]
pub enum Attr {
    /// A function. Tracks whether the function body has been seen (`defined`),
    /// which allows forward declarations and mutual recursion.
    Fun {
        defined: bool,
        bytes_required: usize, // Bytes required is currently not needed. However, left in
        global: bool,
    },
    /// A record type definition, holding its field layout.
    Record(RecordEntry),
    Global,
    Local,
}

/// Describes the field layout of a record type.
///
/// Members are stored in declaration order. Each member has a name,
/// a type, and a byte offset within the record.
#[derive(Debug, Clone)]
pub struct RecordEntry {
    pub members: Vec<(String, MemberEntry)>,
    /// Alignment of the record: the widest alignment of any of its fields.
    pub align: usize,
}

/// A single field within a record.
#[derive(Debug, Clone)]
pub struct MemberEntry {
    /// The type of this field.
    pub member_t: Type,
    /// Byte offset of this field from the start of the record.
    pub offset: usize,
}

impl RecordEntry {
    /// Looks up a member by name.
    pub fn find(&self, name: &str) -> Option<&MemberEntry> {
        self.members
            .iter()
            .find_map(|(mem_name, entry)| if name == mem_name { Some(entry) } else { None })
    }

    /// Looks up a member by its byte offset.
    pub fn find_at_offset(&self, offset: usize) -> Option<&MemberEntry> {
        self.members.iter().find_map(|(_, entry)| {
            if offset == entry.offset {
                Some(entry)
            } else {
                None
            }
        })
    }

    /// Total byte size of the record: the end of the last field, rounded up to
    /// the record's alignment so that an array of them stays aligned.
    pub fn size(&self) -> usize {
        match self.members.last() {
            Some((_, mem)) => {
                let end = mem.offset + crate::types::get_size(&mem.member_t);
                crate::util::round_to_n(end as isize, self.align as isize) as usize
            }
            None => 0, // If there is no last then it takes no space
        }
    }
}

/// A symbol table entry, pairing a [`Type`] with an [`Attr`].
#[derive(Debug, Clone)]
pub struct Entry {
    pub t: Type,
    pub attr: Attr,
}

/// Maps identifier names to their types and attributes.
///
/// There is a single flat namespace — after the resolver runs, all variable
/// names are unique, so scoping does not need to be handled here.
#[derive(Debug, Clone)]
pub struct SymbolTable {
    symbols: HashMap<String, Entry>, // e.g. identifier → type
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolTable {
    pub fn new() -> Self {
        SymbolTable {
            symbols: HashMap::new(),
        }
    }

    /// Applies a mutation function to an existing entry.
    pub fn modify<F>(&mut self, k: String, f: F)
    where
        F: FnOnce(&mut Entry),
    {
        self.symbols.entry(k).and_modify(f);
    }

    /// Inserts a local variable binding.
    pub fn add_local_var(&mut self, name: String, t: Type) {
        self.symbols.insert(
            name,
            Entry {
                t,
                attr: Attr::Local,
            },
        );
    }

    /// Inserts a global variable binding.
    pub fn add_global_var(&mut self, name: String, t: Type) {
        self.symbols.insert(
            name,
            Entry {
                t,
                attr: Attr::Global,
            },
        );
    }

    /// Inserts a function binding. `defined` is `false` for forward declarations
    /// and set to `true` once the function body is typechecked.
    pub fn add_fun(&mut self, name: String, t: Type, defined: bool) {
        self.symbols.insert(
            name,
            Entry {
                t,
                attr: Attr::Fun {
                    defined,
                    bytes_required: 0,
                    global: true,
                },
            },
        );
    }

    /// Inserts a record type definition.
    pub fn add_record(&mut self, name: String, rd: RecordEntry) {
        self.symbols.insert(
            name.clone(),
            Entry {
                t: Type::Record(name),
                attr: Attr::Record(rd),
            },
        );
    }

    /// Returns the entry for `name`.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not in the table. After resolution, all referenced
    /// names are guaranteed to exist, so a missing entry indicates an internal error.
    pub fn get(&self, name: &str) -> &Entry {
        match self.symbols.get(name) {
            Some(sym) => sym,
            None => panic!("Symbol not found in table!"),
        }
    }

    /// Returns the [`Attr`] for `name`.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not in the table.
    pub fn get_attr(&self, name: &str) -> &Attr {
        match self.symbols.get(name) {
            Some(sym) => &sym.attr,
            None => panic!("Symbol {} not found in table!", name),
        }
    }

    /// Returns the [`RecordEntry`] for a record type.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not a record.
    pub fn get_record(&self, name: &str) -> &RecordEntry {
        match self.symbols.get(name) {
            Some(sym) => match &sym.attr {
                Attr::Record(record_entry) => record_entry,
                _ => panic!("Record has non-Record attribute"),
            },
            None => panic!("Record Symbol not found in table!"),
        }
    }

    /// Returns the entry for `name`, or `None` if it does not exist.
    pub fn get_opt(&self, name: &str) -> Option<&Entry> {
        self.symbols.get(name)
    }

    /// Prints all symbols to stdout. Useful for debugging.
    pub fn print_symbols(&self) {
        for (key, val) in self.symbols.iter() {
            println!("K : {key}, V : {:?}", val);
        }
    }
}
