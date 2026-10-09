//! How one language's code reads, by file name.

/// A language's comments, keywords and constants.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Syntax {
    /// The mark that opens a line comment.
    pub comment: &'static str,
    /// The mark counts only where a word starts (Shell).
    pub word_comment: bool,
    pub keywords: &'static [&'static str],
    pub constants: &'static [&'static str],
}

/// C, Rust, JavaScript, Go, Java, Swift and kin.
pub(crate) const C_LIKE: Syntax = Syntax {
    comment: "//",
    word_comment: false,
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "fn",
        "for",
        "from",
        "func",
        "function",
        "if",
        "impl",
        "import",
        "in",
        "interface",
        "let",
        "match",
        "mut",
        "new",
        "package",
        "pub",
        "return",
        "static",
        "struct",
        "switch",
        "throw",
        "trait",
        "try",
        "type",
        "use",
        "var",
        "where",
        "while",
        "yield",
    ],
    constants: &[
        "false",
        "nil",
        "null",
        "self",
        "Self",
        "this",
        "true",
        "undefined",
        "None",
        "Some",
    ],
};

const PYTHON: Syntax = Syntax {
    comment: "#",
    word_comment: false,
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is",
        "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
        "yield",
    ],
    constants: &["True", "False", "None", "self"],
};

const SHELL: Syntax = Syntax {
    comment: "#",
    word_comment: true,
    keywords: &[
        "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if",
        "in", "local", "return", "then", "until", "while",
    ],
    constants: &["true", "false"],
};

/// TOML, YAML, INI and other settings: keys are not words.
const SETTINGS: Syntax = Syntax {
    comment: "#",
    word_comment: true,
    keywords: &[],
    constants: &["true", "false", "null", "yes", "no", "on", "off"],
};

const INI: Syntax = Syntax {
    comment: ";",
    ..SETTINGS
};

const SQL: Syntax = Syntax {
    comment: "--",
    word_comment: false,
    keywords: &[
        "SELECT", "FROM", "WHERE", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "ON", "AND", "OR",
        "NOT", "AS", "GROUP", "BY", "ORDER", "HAVING", "LIMIT", "OFFSET", "INSERT", "INTO",
        "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "INDEX", "DROP", "ALTER", "DESC",
        "ASC", "DISTINCT", "UNION", "WITH", "CASE", "WHEN", "THEN", "ELSE", "END", "IS", "IN",
        "LIKE", "BETWEEN", "EXISTS",
    ],
    constants: &["NULL", "TRUE", "FALSE"],
};

const LUA: Syntax = Syntax {
    comment: "--",
    word_comment: false,
    keywords: &[
        "and", "break", "do", "else", "elseif", "end", "for", "function", "goto", "if", "in",
        "local", "not", "or", "repeat", "return", "then", "until", "while",
    ],
    constants: &["true", "false", "nil"],
};

const LISP: Syntax = Syntax {
    comment: ";",
    word_comment: false,
    keywords: &[
        "defun", "defn", "define", "lambda", "let", "if", "cond", "quote",
    ],
    constants: &["nil", "t", "true", "false"],
};

/// Ruby, Perl, CMake and other `#` languages.
const HASH: Syntax = Syntax {
    comment: "#",
    ..C_LIKE
};

const TEX: Syntax = Syntax {
    comment: "%",
    word_comment: false,
    keywords: &[],
    constants: &[],
};

/// Extensions of prose and plain text, which read plain.
const PROSE: &[&str] = &[
    "md", "markdown", "mdown", "mkd", "txt", "text", "rst", "adoc", "asciidoc", "org", "csv",
    "tsv", "log",
];

/// Languages by extension; any other code reads C-like.
const BY_EXTENSION: &[(&Syntax, &[&str])] = &[
    (&PYTHON, &["py", "pyi", "pyw"]),
    (&SHELL, &["sh", "bash", "zsh", "fish", "ksh"]),
    (
        &HASH,
        &[
            "rb", "pl", "r", "cmake", "mk", "nix", "ex", "exs", "jl", "nim", "cr", "tcl", "ps1",
        ],
    ),
    (
        &SETTINGS,
        &[
            "toml",
            "yaml",
            "yml",
            "conf",
            "cfg",
            "gitignore",
            "env",
            "properties",
        ],
    ),
    (&INI, &["ini"]),
    (&SQL, &["sql"]),
    (&LUA, &["lua"]),
    (&LISP, &["clj", "cljs", "lisp", "el", "scm"]),
    (&TEX, &["tex", "sty", "bib"]),
];

/// How the file at `path` reads; none for prose.
pub(crate) fn syntax_of(path: &str) -> Option<&'static Syntax> {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    // Named files first: CMakeLists.txt is not prose.
    match name {
        "Makefile" | "GNUmakefile" | "Dockerfile" | "CMakeLists.txt" => return Some(&SHELL),
        _ => {}
    }
    let ext = match name.rsplit_once('.') {
        Some((_, ext)) => ext.to_ascii_lowercase(),
        None => return None,
    };
    if PROSE.contains(&ext.as_str()) {
        return None;
    }
    let found = BY_EXTENSION
        .iter()
        .find(|(_, exts)| exts.contains(&ext.as_str()));
    Some(found.map_or(&C_LIKE, |(syntax, _)| *syntax))
}
