//! HTML character references: every named one that stands for a single
//! character (all of HTML 4's, `&apos;` and the rest of HTML5's), and
//! numeric references.

use std::collections::HashMap;
use std::sync::OnceLock;

use doc_encoding::Encoding;

/// The Latin-1 entities, which name U+00A0 to U+00FF in order. HTML also
/// reads these without their closing semicolon.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy",
    "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm", "raquo",
    "frac14", "frac12", "frac34", "iquest", "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil",
    "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve", "Oacute",
    "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN", "szlig",
    "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc", "euml",
    "igrave", "iacute", "icirc", "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml", "divide",
    "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
];

/// Other legacy names HTML reads without a semicolon.
const LEGACY: [&str; 10] = ["amp", "lt", "gt", "quot", "AMP", "LT", "GT", "QUOT", "COPY", "REG"];

/// Every other HTML 4 name and its code point, and `&apos;`.
const NAMED: &[(&str, u32)] = &[
    // HTML 4 special characters.
    ("quot", 0x22),
    ("amp", 0x26),
    ("apos", 0x27),
    ("lt", 0x3C),
    ("gt", 0x3E),
    ("OElig", 0x152),
    ("oelig", 0x153),
    ("Scaron", 0x160),
    ("scaron", 0x161),
    ("Yuml", 0x178),
    ("circ", 0x2C6),
    ("tilde", 0x2DC),
    ("ensp", 0x2002),
    ("emsp", 0x2003),
    ("thinsp", 0x2009),
    ("zwnj", 0x200C),
    ("zwj", 0x200D),
    ("lrm", 0x200E),
    ("rlm", 0x200F),
    ("ndash", 0x2013),
    ("mdash", 0x2014),
    ("lsquo", 0x2018),
    ("rsquo", 0x2019),
    ("sbquo", 0x201A),
    ("ldquo", 0x201C),
    ("rdquo", 0x201D),
    ("bdquo", 0x201E),
    ("dagger", 0x2020),
    ("Dagger", 0x2021),
    ("permil", 0x2030),
    ("lsaquo", 0x2039),
    ("rsaquo", 0x203A),
    ("euro", 0x20AC),
    // HTML 4 symbols: Greek, punctuation, letter-like, arrows, mathematics.
    ("fnof", 0x192),
    ("Alpha", 0x391),
    ("Beta", 0x392),
    ("Gamma", 0x393),
    ("Delta", 0x394),
    ("Epsilon", 0x395),
    ("Zeta", 0x396),
    ("Eta", 0x397),
    ("Theta", 0x398),
    ("Iota", 0x399),
    ("Kappa", 0x39A),
    ("Lambda", 0x39B),
    ("Mu", 0x39C),
    ("Nu", 0x39D),
    ("Xi", 0x39E),
    ("Omicron", 0x39F),
    ("Pi", 0x3A0),
    ("Rho", 0x3A1),
    ("Sigma", 0x3A3),
    ("Tau", 0x3A4),
    ("Upsilon", 0x3A5),
    ("Phi", 0x3A6),
    ("Chi", 0x3A7),
    ("Psi", 0x3A8),
    ("Omega", 0x3A9),
    ("alpha", 0x3B1),
    ("beta", 0x3B2),
    ("gamma", 0x3B3),
    ("delta", 0x3B4),
    ("epsilon", 0x3B5),
    ("zeta", 0x3B6),
    ("eta", 0x3B7),
    ("theta", 0x3B8),
    ("iota", 0x3B9),
    ("kappa", 0x3BA),
    ("lambda", 0x3BB),
    ("mu", 0x3BC),
    ("nu", 0x3BD),
    ("xi", 0x3BE),
    ("omicron", 0x3BF),
    ("pi", 0x3C0),
    ("rho", 0x3C1),
    ("sigmaf", 0x3C2),
    ("sigma", 0x3C3),
    ("tau", 0x3C4),
    ("upsilon", 0x3C5),
    ("phi", 0x3C6),
    ("chi", 0x3C7),
    ("psi", 0x3C8),
    ("omega", 0x3C9),
    ("thetasym", 0x3D1),
    ("upsih", 0x3D2),
    ("piv", 0x3D6),
    ("bull", 0x2022),
    ("hellip", 0x2026),
    ("prime", 0x2032),
    ("Prime", 0x2033),
    ("oline", 0x203E),
    ("frasl", 0x2044),
    ("weierp", 0x2118),
    ("image", 0x2111),
    ("real", 0x211C),
    ("trade", 0x2122),
    ("alefsym", 0x2135),
    ("larr", 0x2190),
    ("uarr", 0x2191),
    ("rarr", 0x2192),
    ("darr", 0x2193),
    ("harr", 0x2194),
    ("crarr", 0x21B5),
    ("lArr", 0x21D0),
    ("uArr", 0x21D1),
    ("rArr", 0x21D2),
    ("dArr", 0x21D3),
    ("hArr", 0x21D4),
    ("forall", 0x2200),
    ("part", 0x2202),
    ("exist", 0x2203),
    ("empty", 0x2205),
    ("nabla", 0x2207),
    ("isin", 0x2208),
    ("notin", 0x2209),
    ("ni", 0x220B),
    ("prod", 0x220F),
    ("sum", 0x2211),
    ("minus", 0x2212),
    ("lowast", 0x2217),
    ("radic", 0x221A),
    ("prop", 0x221D),
    ("infin", 0x221E),
    ("ang", 0x2220),
    ("and", 0x2227),
    ("or", 0x2228),
    ("cap", 0x2229),
    ("cup", 0x222A),
    ("int", 0x222B),
    ("there4", 0x2234),
    ("sim", 0x223C),
    ("cong", 0x2245),
    ("asymp", 0x2248),
    ("ne", 0x2260),
    ("equiv", 0x2261),
    ("le", 0x2264),
    ("ge", 0x2265),
    ("sub", 0x2282),
    ("sup", 0x2283),
    ("nsub", 0x2284),
    ("sube", 0x2286),
    ("supe", 0x2287),
    ("oplus", 0x2295),
    ("otimes", 0x2297),
    ("perp", 0x22A5),
    ("sdot", 0x22C5),
    ("lceil", 0x2308),
    ("rceil", 0x2309),
    ("lfloor", 0x230A),
    ("rfloor", 0x230B),
    // HTML5 moved these two from the deprecated U+2329 and U+232A.
    ("lang", 0x27E8),
    ("rang", 0x27E9),
    ("loz", 0x25CA),
    ("spades", 0x2660),
    ("clubs", 0x2663),
    ("hearts", 0x2665),
    ("diams", 0x2666),
];

/// HTML5's other names for single characters, after
/// <https://html.spec.whatwg.org/entities.json>. The 93 names for two
/// characters (mathematical symbols with a combining mark, such as
/// `&ngE;`) are left out, and stay as written.
const HTML5: &[(&str, u32)] = &[
    ("Abreve", 0x102), ("abreve", 0x103), ("ac", 0x223E), ("acd", 0x223F), ("Acy", 0x410), ("acy", 0x430),
    ("af", 0x2061), ("Afr", 0x1D504), ("afr", 0x1D51E), ("aleph", 0x2135), ("Amacr", 0x100), ("amacr", 0x101),
    ("amalg", 0x2A3F), ("AMP", 0x26), ("And", 0x2A53), ("andand", 0x2A55), ("andd", 0x2A5C), ("andslope", 0x2A58),
    ("andv", 0x2A5A), ("ange", 0x29A4), ("angle", 0x2220), ("angmsd", 0x2221), ("angmsdaa", 0x29A8),
    ("angmsdab", 0x29A9), ("angmsdac", 0x29AA), ("angmsdad", 0x29AB), ("angmsdae", 0x29AC), ("angmsdaf", 0x29AD),
    ("angmsdag", 0x29AE), ("angmsdah", 0x29AF), ("angrt", 0x221F), ("angrtvb", 0x22BE), ("angrtvbd", 0x299D),
    ("angsph", 0x2222), ("angst", 0xC5), ("angzarr", 0x237C), ("Aogon", 0x104), ("aogon", 0x105), ("Aopf", 0x1D538),
    ("aopf", 0x1D552), ("ap", 0x2248), ("apacir", 0x2A6F), ("apE", 0x2A70), ("ape", 0x224A), ("apid", 0x224B),
    ("ApplyFunction", 0x2061), ("approx", 0x2248), ("approxeq", 0x224A), ("Ascr", 0x1D49C), ("ascr", 0x1D4B6),
    ("Assign", 0x2254), ("ast", 0x2A), ("asympeq", 0x224D), ("awconint", 0x2233), ("awint", 0x2A11),
    ("backcong", 0x224C), ("backepsilon", 0x3F6), ("backprime", 0x2035), ("backsim", 0x223D), ("backsimeq", 0x22CD),
    ("Backslash", 0x2216), ("Barv", 0x2AE7), ("barvee", 0x22BD), ("Barwed", 0x2306), ("barwed", 0x2305),
    ("barwedge", 0x2305), ("bbrk", 0x23B5), ("bbrktbrk", 0x23B6), ("bcong", 0x224C), ("Bcy", 0x411), ("bcy", 0x431),
    ("becaus", 0x2235), ("Because", 0x2235), ("because", 0x2235), ("bemptyv", 0x29B0), ("bepsi", 0x3F6),
    ("bernou", 0x212C), ("Bernoullis", 0x212C), ("beth", 0x2136), ("between", 0x226C), ("Bfr", 0x1D505),
    ("bfr", 0x1D51F), ("bigcap", 0x22C2), ("bigcirc", 0x25EF), ("bigcup", 0x22C3), ("bigodot", 0x2A00),
    ("bigoplus", 0x2A01), ("bigotimes", 0x2A02), ("bigsqcup", 0x2A06), ("bigstar", 0x2605),
    ("bigtriangledown", 0x25BD), ("bigtriangleup", 0x25B3), ("biguplus", 0x2A04), ("bigvee", 0x22C1),
    ("bigwedge", 0x22C0), ("bkarow", 0x290D), ("blacklozenge", 0x29EB), ("blacksquare", 0x25AA),
    ("blacktriangle", 0x25B4), ("blacktriangledown", 0x25BE), ("blacktriangleleft", 0x25C2),
    ("blacktriangleright", 0x25B8), ("blank", 0x2423), ("blk12", 0x2592), ("blk14", 0x2591), ("blk34", 0x2593),
    ("block", 0x2588), ("bNot", 0x2AED), ("bnot", 0x2310), ("Bopf", 0x1D539), ("bopf", 0x1D553), ("bot", 0x22A5),
    ("bottom", 0x22A5), ("bowtie", 0x22C8), ("boxbox", 0x29C9), ("boxDL", 0x2557), ("boxDl", 0x2556),
    ("boxdL", 0x2555), ("boxdl", 0x2510), ("boxDR", 0x2554), ("boxDr", 0x2553), ("boxdR", 0x2552), ("boxdr", 0x250C),
    ("boxH", 0x2550), ("boxh", 0x2500), ("boxHD", 0x2566), ("boxHd", 0x2564), ("boxhD", 0x2565), ("boxhd", 0x252C),
    ("boxHU", 0x2569), ("boxHu", 0x2567), ("boxhU", 0x2568), ("boxhu", 0x2534), ("boxminus", 0x229F),
    ("boxplus", 0x229E), ("boxtimes", 0x22A0), ("boxUL", 0x255D), ("boxUl", 0x255C), ("boxuL", 0x255B),
    ("boxul", 0x2518), ("boxUR", 0x255A), ("boxUr", 0x2559), ("boxuR", 0x2558), ("boxur", 0x2514), ("boxV", 0x2551),
    ("boxv", 0x2502), ("boxVH", 0x256C), ("boxVh", 0x256B), ("boxvH", 0x256A), ("boxvh", 0x253C), ("boxVL", 0x2563),
    ("boxVl", 0x2562), ("boxvL", 0x2561), ("boxvl", 0x2524), ("boxVR", 0x2560), ("boxVr", 0x255F), ("boxvR", 0x255E),
    ("boxvr", 0x251C), ("bprime", 0x2035), ("Breve", 0x2D8), ("breve", 0x2D8), ("Bscr", 0x212C), ("bscr", 0x1D4B7),
    ("bsemi", 0x204F), ("bsim", 0x223D), ("bsime", 0x22CD), ("bsol", 0x5C), ("bsolb", 0x29C5), ("bsolhsub", 0x27C8),
    ("bullet", 0x2022), ("bump", 0x224E), ("bumpE", 0x2AAE), ("bumpe", 0x224F), ("Bumpeq", 0x224E),
    ("bumpeq", 0x224F), ("Cacute", 0x106), ("cacute", 0x107), ("Cap", 0x22D2), ("capand", 0x2A44),
    ("capbrcup", 0x2A49), ("capcap", 0x2A4B), ("capcup", 0x2A47), ("capdot", 0x2A40),
    ("CapitalDifferentialD", 0x2145), ("caret", 0x2041), ("caron", 0x2C7), ("Cayleys", 0x212D), ("ccaps", 0x2A4D),
    ("Ccaron", 0x10C), ("ccaron", 0x10D), ("Ccirc", 0x108), ("ccirc", 0x109), ("Cconint", 0x2230), ("ccups", 0x2A4C),
    ("ccupssm", 0x2A50), ("Cdot", 0x10A), ("cdot", 0x10B), ("Cedilla", 0xB8), ("cemptyv", 0x29B2),
    ("CenterDot", 0xB7), ("centerdot", 0xB7), ("Cfr", 0x212D), ("cfr", 0x1D520), ("CHcy", 0x427), ("chcy", 0x447),
    ("check", 0x2713), ("checkmark", 0x2713), ("cir", 0x25CB), ("circeq", 0x2257), ("circlearrowleft", 0x21BA),
    ("circlearrowright", 0x21BB), ("circledast", 0x229B), ("circledcirc", 0x229A), ("circleddash", 0x229D),
    ("CircleDot", 0x2299), ("circledR", 0xAE), ("circledS", 0x24C8), ("CircleMinus", 0x2296), ("CirclePlus", 0x2295),
    ("CircleTimes", 0x2297), ("cirE", 0x29C3), ("cire", 0x2257), ("cirfnint", 0x2A10), ("cirmid", 0x2AEF),
    ("cirscir", 0x29C2), ("ClockwiseContourIntegral", 0x2232), ("CloseCurlyDoubleQuote", 0x201D),
    ("CloseCurlyQuote", 0x2019), ("clubsuit", 0x2663), ("Colon", 0x2237), ("colon", 0x3A), ("Colone", 0x2A74),
    ("colone", 0x2254), ("coloneq", 0x2254), ("comma", 0x2C), ("commat", 0x40), ("comp", 0x2201), ("compfn", 0x2218),
    ("complement", 0x2201), ("complexes", 0x2102), ("congdot", 0x2A6D), ("Congruent", 0x2261), ("Conint", 0x222F),
    ("conint", 0x222E), ("ContourIntegral", 0x222E), ("Copf", 0x2102), ("copf", 0x1D554), ("coprod", 0x2210),
    ("Coproduct", 0x2210), ("COPY", 0xA9), ("copysr", 0x2117), ("CounterClockwiseContourIntegral", 0x2233),
    ("Cross", 0x2A2F), ("cross", 0x2717), ("Cscr", 0x1D49E), ("cscr", 0x1D4B8), ("csub", 0x2ACF), ("csube", 0x2AD1),
    ("csup", 0x2AD0), ("csupe", 0x2AD2), ("ctdot", 0x22EF), ("cudarrl", 0x2938), ("cudarrr", 0x2935),
    ("cuepr", 0x22DE), ("cuesc", 0x22DF), ("cularr", 0x21B6), ("cularrp", 0x293D), ("Cup", 0x22D3),
    ("cupbrcap", 0x2A48), ("CupCap", 0x224D), ("cupcap", 0x2A46), ("cupcup", 0x2A4A), ("cupdot", 0x228D),
    ("cupor", 0x2A45), ("curarr", 0x21B7), ("curarrm", 0x293C), ("curlyeqprec", 0x22DE), ("curlyeqsucc", 0x22DF),
    ("curlyvee", 0x22CE), ("curlywedge", 0x22CF), ("curvearrowleft", 0x21B6), ("curvearrowright", 0x21B7),
    ("cuvee", 0x22CE), ("cuwed", 0x22CF), ("cwconint", 0x2232), ("cwint", 0x2231), ("cylcty", 0x232D),
    ("daleth", 0x2138), ("Darr", 0x21A1), ("dash", 0x2010), ("Dashv", 0x2AE4), ("dashv", 0x22A3), ("dbkarow", 0x290F),
    ("dblac", 0x2DD), ("Dcaron", 0x10E), ("dcaron", 0x10F), ("Dcy", 0x414), ("dcy", 0x434), ("DD", 0x2145),
    ("dd", 0x2146), ("ddagger", 0x2021), ("ddarr", 0x21CA), ("DDotrahd", 0x2911), ("ddotseq", 0x2A77),
    ("Del", 0x2207), ("demptyv", 0x29B1), ("dfisht", 0x297F), ("Dfr", 0x1D507), ("dfr", 0x1D521), ("dHar", 0x2965),
    ("dharl", 0x21C3), ("dharr", 0x21C2), ("DiacriticalAcute", 0xB4), ("DiacriticalDot", 0x2D9),
    ("DiacriticalDoubleAcute", 0x2DD), ("DiacriticalGrave", 0x60), ("DiacriticalTilde", 0x2DC), ("diam", 0x22C4),
    ("Diamond", 0x22C4), ("diamond", 0x22C4), ("diamondsuit", 0x2666), ("die", 0xA8), ("DifferentialD", 0x2146),
    ("digamma", 0x3DD), ("disin", 0x22F2), ("div", 0xF7), ("divideontimes", 0x22C7), ("divonx", 0x22C7),
    ("DJcy", 0x402), ("djcy", 0x452), ("dlcorn", 0x231E), ("dlcrop", 0x230D), ("dollar", 0x24), ("Dopf", 0x1D53B),
    ("dopf", 0x1D555), ("Dot", 0xA8), ("dot", 0x2D9), ("DotDot", 0x20DC), ("doteq", 0x2250), ("doteqdot", 0x2251),
    ("DotEqual", 0x2250), ("dotminus", 0x2238), ("dotplus", 0x2214), ("dotsquare", 0x22A1),
    ("doublebarwedge", 0x2306), ("DoubleContourIntegral", 0x222F), ("DoubleDot", 0xA8), ("DoubleDownArrow", 0x21D3),
    ("DoubleLeftArrow", 0x21D0), ("DoubleLeftRightArrow", 0x21D4), ("DoubleLeftTee", 0x2AE4),
    ("DoubleLongLeftArrow", 0x27F8), ("DoubleLongLeftRightArrow", 0x27FA), ("DoubleLongRightArrow", 0x27F9),
    ("DoubleRightArrow", 0x21D2), ("DoubleRightTee", 0x22A8), ("DoubleUpArrow", 0x21D1),
    ("DoubleUpDownArrow", 0x21D5), ("DoubleVerticalBar", 0x2225), ("DownArrow", 0x2193), ("Downarrow", 0x21D3),
    ("downarrow", 0x2193), ("DownArrowBar", 0x2913), ("DownArrowUpArrow", 0x21F5), ("DownBreve", 0x311),
    ("downdownarrows", 0x21CA), ("downharpoonleft", 0x21C3), ("downharpoonright", 0x21C2),
    ("DownLeftRightVector", 0x2950), ("DownLeftTeeVector", 0x295E), ("DownLeftVector", 0x21BD),
    ("DownLeftVectorBar", 0x2956), ("DownRightTeeVector", 0x295F), ("DownRightVector", 0x21C1),
    ("DownRightVectorBar", 0x2957), ("DownTee", 0x22A4), ("DownTeeArrow", 0x21A7), ("drbkarow", 0x2910),
    ("drcorn", 0x231F), ("drcrop", 0x230C), ("Dscr", 0x1D49F), ("dscr", 0x1D4B9), ("DScy", 0x405), ("dscy", 0x455),
    ("dsol", 0x29F6), ("Dstrok", 0x110), ("dstrok", 0x111), ("dtdot", 0x22F1), ("dtri", 0x25BF), ("dtrif", 0x25BE),
    ("duarr", 0x21F5), ("duhar", 0x296F), ("dwangle", 0x29A6), ("DZcy", 0x40F), ("dzcy", 0x45F), ("dzigrarr", 0x27FF),
    ("easter", 0x2A6E), ("Ecaron", 0x11A), ("ecaron", 0x11B), ("ecir", 0x2256), ("ecolon", 0x2255), ("Ecy", 0x42D),
    ("ecy", 0x44D), ("eDDot", 0x2A77), ("Edot", 0x116), ("eDot", 0x2251), ("edot", 0x117), ("ee", 0x2147),
    ("efDot", 0x2252), ("Efr", 0x1D508), ("efr", 0x1D522), ("eg", 0x2A9A), ("egs", 0x2A96), ("egsdot", 0x2A98),
    ("el", 0x2A99), ("Element", 0x2208), ("elinters", 0x23E7), ("ell", 0x2113), ("els", 0x2A95), ("elsdot", 0x2A97),
    ("Emacr", 0x112), ("emacr", 0x113), ("emptyset", 0x2205), ("EmptySmallSquare", 0x25FB), ("emptyv", 0x2205),
    ("EmptyVerySmallSquare", 0x25AB), ("emsp13", 0x2004), ("emsp14", 0x2005), ("ENG", 0x14A), ("eng", 0x14B),
    ("Eogon", 0x118), ("eogon", 0x119), ("Eopf", 0x1D53C), ("eopf", 0x1D556), ("epar", 0x22D5), ("eparsl", 0x29E3),
    ("eplus", 0x2A71), ("epsi", 0x3B5), ("epsiv", 0x3F5), ("eqcirc", 0x2256), ("eqcolon", 0x2255), ("eqsim", 0x2242),
    ("eqslantgtr", 0x2A96), ("eqslantless", 0x2A95), ("Equal", 0x2A75), ("equals", 0x3D), ("EqualTilde", 0x2242),
    ("equest", 0x225F), ("Equilibrium", 0x21CC), ("equivDD", 0x2A78), ("eqvparsl", 0x29E5), ("erarr", 0x2971),
    ("erDot", 0x2253), ("Escr", 0x2130), ("escr", 0x212F), ("esdot", 0x2250), ("Esim", 0x2A73), ("esim", 0x2242),
    ("excl", 0x21), ("Exists", 0x2203), ("expectation", 0x2130), ("ExponentialE", 0x2147), ("exponentiale", 0x2147),
    ("fallingdotseq", 0x2252), ("Fcy", 0x424), ("fcy", 0x444), ("female", 0x2640), ("ffilig", 0xFB03),
    ("fflig", 0xFB00), ("ffllig", 0xFB04), ("Ffr", 0x1D509), ("ffr", 0x1D523), ("filig", 0xFB01),
    ("FilledSmallSquare", 0x25FC), ("FilledVerySmallSquare", 0x25AA), ("flat", 0x266D), ("fllig", 0xFB02),
    ("fltns", 0x25B1), ("Fopf", 0x1D53D), ("fopf", 0x1D557), ("ForAll", 0x2200), ("fork", 0x22D4), ("forkv", 0x2AD9),
    ("Fouriertrf", 0x2131), ("fpartint", 0x2A0D), ("frac13", 0x2153), ("frac15", 0x2155), ("frac16", 0x2159),
    ("frac18", 0x215B), ("frac23", 0x2154), ("frac25", 0x2156), ("frac35", 0x2157), ("frac38", 0x215C),
    ("frac45", 0x2158), ("frac56", 0x215A), ("frac58", 0x215D), ("frac78", 0x215E), ("frown", 0x2322),
    ("Fscr", 0x2131), ("fscr", 0x1D4BB), ("gacute", 0x1F5), ("Gammad", 0x3DC), ("gammad", 0x3DD), ("gap", 0x2A86),
    ("Gbreve", 0x11E), ("gbreve", 0x11F), ("Gcedil", 0x122), ("Gcirc", 0x11C), ("gcirc", 0x11D), ("Gcy", 0x413),
    ("gcy", 0x433), ("Gdot", 0x120), ("gdot", 0x121), ("gE", 0x2267), ("gEl", 0x2A8C), ("gel", 0x22DB),
    ("geq", 0x2265), ("geqq", 0x2267), ("geqslant", 0x2A7E), ("ges", 0x2A7E), ("gescc", 0x2AA9), ("gesdot", 0x2A80),
    ("gesdoto", 0x2A82), ("gesdotol", 0x2A84), ("gesles", 0x2A94), ("Gfr", 0x1D50A), ("gfr", 0x1D524), ("Gg", 0x22D9),
    ("gg", 0x226B), ("ggg", 0x22D9), ("gimel", 0x2137), ("GJcy", 0x403), ("gjcy", 0x453), ("gl", 0x2277),
    ("gla", 0x2AA5), ("glE", 0x2A92), ("glj", 0x2AA4), ("gnap", 0x2A8A), ("gnapprox", 0x2A8A), ("gnE", 0x2269),
    ("gne", 0x2A88), ("gneq", 0x2A88), ("gneqq", 0x2269), ("gnsim", 0x22E7), ("Gopf", 0x1D53E), ("gopf", 0x1D558),
    ("grave", 0x60), ("GreaterEqual", 0x2265), ("GreaterEqualLess", 0x22DB), ("GreaterFullEqual", 0x2267),
    ("GreaterGreater", 0x2AA2), ("GreaterLess", 0x2277), ("GreaterSlantEqual", 0x2A7E), ("GreaterTilde", 0x2273),
    ("Gscr", 0x1D4A2), ("gscr", 0x210A), ("gsim", 0x2273), ("gsime", 0x2A8E), ("gsiml", 0x2A90), ("GT", 0x3E),
    ("Gt", 0x226B), ("gtcc", 0x2AA7), ("gtcir", 0x2A7A), ("gtdot", 0x22D7), ("gtlPar", 0x2995), ("gtquest", 0x2A7C),
    ("gtrapprox", 0x2A86), ("gtrarr", 0x2978), ("gtrdot", 0x22D7), ("gtreqless", 0x22DB), ("gtreqqless", 0x2A8C),
    ("gtrless", 0x2277), ("gtrsim", 0x2273), ("Hacek", 0x2C7), ("hairsp", 0x200A), ("half", 0xBD), ("hamilt", 0x210B),
    ("HARDcy", 0x42A), ("hardcy", 0x44A), ("harrcir", 0x2948), ("harrw", 0x21AD), ("Hat", 0x5E), ("hbar", 0x210F),
    ("Hcirc", 0x124), ("hcirc", 0x125), ("heartsuit", 0x2665), ("hercon", 0x22B9), ("Hfr", 0x210C), ("hfr", 0x1D525),
    ("HilbertSpace", 0x210B), ("hksearow", 0x2925), ("hkswarow", 0x2926), ("hoarr", 0x21FF), ("homtht", 0x223B),
    ("hookleftarrow", 0x21A9), ("hookrightarrow", 0x21AA), ("Hopf", 0x210D), ("hopf", 0x1D559), ("horbar", 0x2015),
    ("HorizontalLine", 0x2500), ("Hscr", 0x210B), ("hscr", 0x1D4BD), ("hslash", 0x210F), ("Hstrok", 0x126),
    ("hstrok", 0x127), ("HumpDownHump", 0x224E), ("HumpEqual", 0x224F), ("hybull", 0x2043), ("hyphen", 0x2010),
    ("ic", 0x2063), ("Icy", 0x418), ("icy", 0x438), ("Idot", 0x130), ("IEcy", 0x415), ("iecy", 0x435),
    ("iff", 0x21D4), ("Ifr", 0x2111), ("ifr", 0x1D526), ("ii", 0x2148), ("iiiint", 0x2A0C), ("iiint", 0x222D),
    ("iinfin", 0x29DC), ("iiota", 0x2129), ("IJlig", 0x132), ("ijlig", 0x133), ("Im", 0x2111), ("Imacr", 0x12A),
    ("imacr", 0x12B), ("ImaginaryI", 0x2148), ("imagline", 0x2110), ("imagpart", 0x2111), ("imath", 0x131),
    ("imof", 0x22B7), ("imped", 0x1B5), ("Implies", 0x21D2), ("in", 0x2208), ("incare", 0x2105), ("infintie", 0x29DD),
    ("inodot", 0x131), ("Int", 0x222C), ("intcal", 0x22BA), ("integers", 0x2124), ("Integral", 0x222B),
    ("intercal", 0x22BA), ("Intersection", 0x22C2), ("intlarhk", 0x2A17), ("intprod", 0x2A3C),
    ("InvisibleComma", 0x2063), ("InvisibleTimes", 0x2062), ("IOcy", 0x401), ("iocy", 0x451), ("Iogon", 0x12E),
    ("iogon", 0x12F), ("Iopf", 0x1D540), ("iopf", 0x1D55A), ("iprod", 0x2A3C), ("Iscr", 0x2110), ("iscr", 0x1D4BE),
    ("isindot", 0x22F5), ("isinE", 0x22F9), ("isins", 0x22F4), ("isinsv", 0x22F3), ("isinv", 0x2208), ("it", 0x2062),
    ("Itilde", 0x128), ("itilde", 0x129), ("Iukcy", 0x406), ("iukcy", 0x456), ("Jcirc", 0x134), ("jcirc", 0x135),
    ("Jcy", 0x419), ("jcy", 0x439), ("Jfr", 0x1D50D), ("jfr", 0x1D527), ("jmath", 0x237), ("Jopf", 0x1D541),
    ("jopf", 0x1D55B), ("Jscr", 0x1D4A5), ("jscr", 0x1D4BF), ("Jsercy", 0x408), ("jsercy", 0x458), ("Jukcy", 0x404),
    ("jukcy", 0x454), ("kappav", 0x3F0), ("Kcedil", 0x136), ("kcedil", 0x137), ("Kcy", 0x41A), ("kcy", 0x43A),
    ("Kfr", 0x1D50E), ("kfr", 0x1D528), ("kgreen", 0x138), ("KHcy", 0x425), ("khcy", 0x445), ("KJcy", 0x40C),
    ("kjcy", 0x45C), ("Kopf", 0x1D542), ("kopf", 0x1D55C), ("Kscr", 0x1D4A6), ("kscr", 0x1D4C0), ("lAarr", 0x21DA),
    ("Lacute", 0x139), ("lacute", 0x13A), ("laemptyv", 0x29B4), ("lagran", 0x2112), ("Lang", 0x27EA),
    ("langd", 0x2991), ("langle", 0x27E8), ("lap", 0x2A85), ("Laplacetrf", 0x2112), ("Larr", 0x219E),
    ("larrb", 0x21E4), ("larrbfs", 0x291F), ("larrfs", 0x291D), ("larrhk", 0x21A9), ("larrlp", 0x21AB),
    ("larrpl", 0x2939), ("larrsim", 0x2973), ("larrtl", 0x21A2), ("lat", 0x2AAB), ("lAtail", 0x291B),
    ("latail", 0x2919), ("late", 0x2AAD), ("lBarr", 0x290E), ("lbarr", 0x290C), ("lbbrk", 0x2772), ("lbrace", 0x7B),
    ("lbrack", 0x5B), ("lbrke", 0x298B), ("lbrksld", 0x298F), ("lbrkslu", 0x298D), ("Lcaron", 0x13D),
    ("lcaron", 0x13E), ("Lcedil", 0x13B), ("lcedil", 0x13C), ("lcub", 0x7B), ("Lcy", 0x41B), ("lcy", 0x43B),
    ("ldca", 0x2936), ("ldquor", 0x201E), ("ldrdhar", 0x2967), ("ldrushar", 0x294B), ("ldsh", 0x21B2), ("lE", 0x2266),
    ("LeftAngleBracket", 0x27E8), ("LeftArrow", 0x2190), ("Leftarrow", 0x21D0), ("leftarrow", 0x2190),
    ("LeftArrowBar", 0x21E4), ("LeftArrowRightArrow", 0x21C6), ("leftarrowtail", 0x21A2), ("LeftCeiling", 0x2308),
    ("LeftDoubleBracket", 0x27E6), ("LeftDownTeeVector", 0x2961), ("LeftDownVector", 0x21C3),
    ("LeftDownVectorBar", 0x2959), ("LeftFloor", 0x230A), ("leftharpoondown", 0x21BD), ("leftharpoonup", 0x21BC),
    ("leftleftarrows", 0x21C7), ("LeftRightArrow", 0x2194), ("Leftrightarrow", 0x21D4), ("leftrightarrow", 0x2194),
    ("leftrightarrows", 0x21C6), ("leftrightharpoons", 0x21CB), ("leftrightsquigarrow", 0x21AD),
    ("LeftRightVector", 0x294E), ("LeftTee", 0x22A3), ("LeftTeeArrow", 0x21A4), ("LeftTeeVector", 0x295A),
    ("leftthreetimes", 0x22CB), ("LeftTriangle", 0x22B2), ("LeftTriangleBar", 0x29CF), ("LeftTriangleEqual", 0x22B4),
    ("LeftUpDownVector", 0x2951), ("LeftUpTeeVector", 0x2960), ("LeftUpVector", 0x21BF), ("LeftUpVectorBar", 0x2958),
    ("LeftVector", 0x21BC), ("LeftVectorBar", 0x2952), ("lEg", 0x2A8B), ("leg", 0x22DA), ("leq", 0x2264),
    ("leqq", 0x2266), ("leqslant", 0x2A7D), ("les", 0x2A7D), ("lescc", 0x2AA8), ("lesdot", 0x2A7F),
    ("lesdoto", 0x2A81), ("lesdotor", 0x2A83), ("lesges", 0x2A93), ("lessapprox", 0x2A85), ("lessdot", 0x22D6),
    ("lesseqgtr", 0x22DA), ("lesseqqgtr", 0x2A8B), ("LessEqualGreater", 0x22DA), ("LessFullEqual", 0x2266),
    ("LessGreater", 0x2276), ("lessgtr", 0x2276), ("LessLess", 0x2AA1), ("lesssim", 0x2272),
    ("LessSlantEqual", 0x2A7D), ("LessTilde", 0x2272), ("lfisht", 0x297C), ("Lfr", 0x1D50F), ("lfr", 0x1D529),
    ("lg", 0x2276), ("lgE", 0x2A91), ("lHar", 0x2962), ("lhard", 0x21BD), ("lharu", 0x21BC), ("lharul", 0x296A),
    ("lhblk", 0x2584), ("LJcy", 0x409), ("ljcy", 0x459), ("Ll", 0x22D8), ("ll", 0x226A), ("llarr", 0x21C7),
    ("llcorner", 0x231E), ("Lleftarrow", 0x21DA), ("llhard", 0x296B), ("lltri", 0x25FA), ("Lmidot", 0x13F),
    ("lmidot", 0x140), ("lmoust", 0x23B0), ("lmoustache", 0x23B0), ("lnap", 0x2A89), ("lnapprox", 0x2A89),
    ("lnE", 0x2268), ("lne", 0x2A87), ("lneq", 0x2A87), ("lneqq", 0x2268), ("lnsim", 0x22E6), ("loang", 0x27EC),
    ("loarr", 0x21FD), ("lobrk", 0x27E6), ("LongLeftArrow", 0x27F5), ("Longleftarrow", 0x27F8),
    ("longleftarrow", 0x27F5), ("LongLeftRightArrow", 0x27F7), ("Longleftrightarrow", 0x27FA),
    ("longleftrightarrow", 0x27F7), ("longmapsto", 0x27FC), ("LongRightArrow", 0x27F6), ("Longrightarrow", 0x27F9),
    ("longrightarrow", 0x27F6), ("looparrowleft", 0x21AB), ("looparrowright", 0x21AC), ("lopar", 0x2985),
    ("Lopf", 0x1D543), ("lopf", 0x1D55D), ("loplus", 0x2A2D), ("lotimes", 0x2A34), ("lowbar", 0x5F),
    ("LowerLeftArrow", 0x2199), ("LowerRightArrow", 0x2198), ("lozenge", 0x25CA), ("lozf", 0x29EB), ("lpar", 0x28),
    ("lparlt", 0x2993), ("lrarr", 0x21C6), ("lrcorner", 0x231F), ("lrhar", 0x21CB), ("lrhard", 0x296D),
    ("lrtri", 0x22BF), ("Lscr", 0x2112), ("lscr", 0x1D4C1), ("Lsh", 0x21B0), ("lsh", 0x21B0), ("lsim", 0x2272),
    ("lsime", 0x2A8D), ("lsimg", 0x2A8F), ("lsqb", 0x5B), ("lsquor", 0x201A), ("Lstrok", 0x141), ("lstrok", 0x142),
    ("LT", 0x3C), ("Lt", 0x226A), ("ltcc", 0x2AA6), ("ltcir", 0x2A79), ("ltdot", 0x22D6), ("lthree", 0x22CB),
    ("ltimes", 0x22C9), ("ltlarr", 0x2976), ("ltquest", 0x2A7B), ("ltri", 0x25C3), ("ltrie", 0x22B4),
    ("ltrif", 0x25C2), ("ltrPar", 0x2996), ("lurdshar", 0x294A), ("luruhar", 0x2966), ("male", 0x2642),
    ("malt", 0x2720), ("maltese", 0x2720), ("Map", 0x2905), ("map", 0x21A6), ("mapsto", 0x21A6),
    ("mapstodown", 0x21A7), ("mapstoleft", 0x21A4), ("mapstoup", 0x21A5), ("marker", 0x25AE), ("mcomma", 0x2A29),
    ("Mcy", 0x41C), ("mcy", 0x43C), ("mDDot", 0x223A), ("measuredangle", 0x2221), ("MediumSpace", 0x205F),
    ("Mellintrf", 0x2133), ("Mfr", 0x1D510), ("mfr", 0x1D52A), ("mho", 0x2127), ("mid", 0x2223), ("midast", 0x2A),
    ("midcir", 0x2AF0), ("minusb", 0x229F), ("minusd", 0x2238), ("minusdu", 0x2A2A), ("MinusPlus", 0x2213),
    ("mlcp", 0x2ADB), ("mldr", 0x2026), ("mnplus", 0x2213), ("models", 0x22A7), ("Mopf", 0x1D544), ("mopf", 0x1D55E),
    ("mp", 0x2213), ("Mscr", 0x2133), ("mscr", 0x1D4C2), ("mstpos", 0x223E), ("multimap", 0x22B8), ("mumap", 0x22B8),
    ("Nacute", 0x143), ("nacute", 0x144), ("nap", 0x2249), ("napos", 0x149), ("napprox", 0x2249), ("natur", 0x266E),
    ("natural", 0x266E), ("naturals", 0x2115), ("ncap", 0x2A43), ("Ncaron", 0x147), ("ncaron", 0x148),
    ("Ncedil", 0x145), ("ncedil", 0x146), ("ncong", 0x2247), ("ncup", 0x2A42), ("Ncy", 0x41D), ("ncy", 0x43D),
    ("nearhk", 0x2924), ("neArr", 0x21D7), ("nearr", 0x2197), ("nearrow", 0x2197), ("NegativeMediumSpace", 0x200B),
    ("NegativeThickSpace", 0x200B), ("NegativeThinSpace", 0x200B), ("NegativeVeryThinSpace", 0x200B),
    ("nequiv", 0x2262), ("nesear", 0x2928), ("NestedGreaterGreater", 0x226B), ("NestedLessLess", 0x226A),
    ("NewLine", 0xA), ("nexist", 0x2204), ("nexists", 0x2204), ("Nfr", 0x1D511), ("nfr", 0x1D52B), ("nge", 0x2271),
    ("ngeq", 0x2271), ("ngsim", 0x2275), ("ngt", 0x226F), ("ngtr", 0x226F), ("nhArr", 0x21CE), ("nharr", 0x21AE),
    ("nhpar", 0x2AF2), ("nis", 0x22FC), ("nisd", 0x22FA), ("niv", 0x220B), ("NJcy", 0x40A), ("njcy", 0x45A),
    ("nlArr", 0x21CD), ("nlarr", 0x219A), ("nldr", 0x2025), ("nle", 0x2270), ("nLeftarrow", 0x21CD),
    ("nleftarrow", 0x219A), ("nLeftrightarrow", 0x21CE), ("nleftrightarrow", 0x21AE), ("nleq", 0x2270),
    ("nless", 0x226E), ("nlsim", 0x2274), ("nlt", 0x226E), ("nltri", 0x22EA), ("nltrie", 0x22EC), ("nmid", 0x2224),
    ("NoBreak", 0x2060), ("NonBreakingSpace", 0xA0), ("Nopf", 0x2115), ("nopf", 0x1D55F), ("Not", 0x2AEC),
    ("NotCongruent", 0x2262), ("NotCupCap", 0x226D), ("NotDoubleVerticalBar", 0x2226), ("NotElement", 0x2209),
    ("NotEqual", 0x2260), ("NotExists", 0x2204), ("NotGreater", 0x226F), ("NotGreaterEqual", 0x2271),
    ("NotGreaterLess", 0x2279), ("NotGreaterTilde", 0x2275), ("notinva", 0x2209), ("notinvb", 0x22F7),
    ("notinvc", 0x22F6), ("NotLeftTriangle", 0x22EA), ("NotLeftTriangleEqual", 0x22EC), ("NotLess", 0x226E),
    ("NotLessEqual", 0x2270), ("NotLessGreater", 0x2278), ("NotLessTilde", 0x2274), ("notni", 0x220C),
    ("notniva", 0x220C), ("notnivb", 0x22FE), ("notnivc", 0x22FD), ("NotPrecedes", 0x2280),
    ("NotPrecedesSlantEqual", 0x22E0), ("NotReverseElement", 0x220C), ("NotRightTriangle", 0x22EB),
    ("NotRightTriangleEqual", 0x22ED), ("NotSquareSubsetEqual", 0x22E2), ("NotSquareSupersetEqual", 0x22E3),
    ("NotSubsetEqual", 0x2288), ("NotSucceeds", 0x2281), ("NotSucceedsSlantEqual", 0x22E1),
    ("NotSupersetEqual", 0x2289), ("NotTilde", 0x2241), ("NotTildeEqual", 0x2244), ("NotTildeFullEqual", 0x2247),
    ("NotTildeTilde", 0x2249), ("NotVerticalBar", 0x2224), ("npar", 0x2226), ("nparallel", 0x2226),
    ("npolint", 0x2A14), ("npr", 0x2280), ("nprcue", 0x22E0), ("nprec", 0x2280), ("nrArr", 0x21CF), ("nrarr", 0x219B),
    ("nRightarrow", 0x21CF), ("nrightarrow", 0x219B), ("nrtri", 0x22EB), ("nrtrie", 0x22ED), ("nsc", 0x2281),
    ("nsccue", 0x22E1), ("Nscr", 0x1D4A9), ("nscr", 0x1D4C3), ("nshortmid", 0x2224), ("nshortparallel", 0x2226),
    ("nsim", 0x2241), ("nsime", 0x2244), ("nsimeq", 0x2244), ("nsmid", 0x2224), ("nspar", 0x2226),
    ("nsqsube", 0x22E2), ("nsqsupe", 0x22E3), ("nsube", 0x2288), ("nsubseteq", 0x2288), ("nsucc", 0x2281),
    ("nsup", 0x2285), ("nsupe", 0x2289), ("nsupseteq", 0x2289), ("ntgl", 0x2279), ("ntlg", 0x2278),
    ("ntriangleleft", 0x22EA), ("ntrianglelefteq", 0x22EC), ("ntriangleright", 0x22EB), ("ntrianglerighteq", 0x22ED),
    ("num", 0x23), ("numero", 0x2116), ("numsp", 0x2007), ("nVDash", 0x22AF), ("nVdash", 0x22AE), ("nvDash", 0x22AD),
    ("nvdash", 0x22AC), ("nvHarr", 0x2904), ("nvinfin", 0x29DE), ("nvlArr", 0x2902), ("nvrArr", 0x2903),
    ("nwarhk", 0x2923), ("nwArr", 0x21D6), ("nwarr", 0x2196), ("nwarrow", 0x2196), ("nwnear", 0x2927),
    ("oast", 0x229B), ("ocir", 0x229A), ("Ocy", 0x41E), ("ocy", 0x43E), ("odash", 0x229D), ("Odblac", 0x150),
    ("odblac", 0x151), ("odiv", 0x2A38), ("odot", 0x2299), ("odsold", 0x29BC), ("ofcir", 0x29BF), ("Ofr", 0x1D512),
    ("ofr", 0x1D52C), ("ogon", 0x2DB), ("ogt", 0x29C1), ("ohbar", 0x29B5), ("ohm", 0x3A9), ("oint", 0x222E),
    ("olarr", 0x21BA), ("olcir", 0x29BE), ("olcross", 0x29BB), ("olt", 0x29C0), ("Omacr", 0x14C), ("omacr", 0x14D),
    ("omid", 0x29B6), ("ominus", 0x2296), ("Oopf", 0x1D546), ("oopf", 0x1D560), ("opar", 0x29B7),
    ("OpenCurlyDoubleQuote", 0x201C), ("OpenCurlyQuote", 0x2018), ("operp", 0x29B9), ("Or", 0x2A54),
    ("orarr", 0x21BB), ("ord", 0x2A5D), ("order", 0x2134), ("orderof", 0x2134), ("origof", 0x22B6), ("oror", 0x2A56),
    ("orslope", 0x2A57), ("orv", 0x2A5B), ("oS", 0x24C8), ("Oscr", 0x1D4AA), ("oscr", 0x2134), ("osol", 0x2298),
    ("Otimes", 0x2A37), ("otimesas", 0x2A36), ("ovbar", 0x233D), ("OverBar", 0x203E), ("OverBrace", 0x23DE),
    ("OverBracket", 0x23B4), ("OverParenthesis", 0x23DC), ("par", 0x2225), ("parallel", 0x2225), ("parsim", 0x2AF3),
    ("parsl", 0x2AFD), ("PartialD", 0x2202), ("Pcy", 0x41F), ("pcy", 0x43F), ("percnt", 0x25), ("period", 0x2E),
    ("pertenk", 0x2031), ("Pfr", 0x1D513), ("pfr", 0x1D52D), ("phiv", 0x3D5), ("phmmat", 0x2133), ("phone", 0x260E),
    ("pitchfork", 0x22D4), ("planck", 0x210F), ("planckh", 0x210E), ("plankv", 0x210F), ("plus", 0x2B),
    ("plusacir", 0x2A23), ("plusb", 0x229E), ("pluscir", 0x2A22), ("plusdo", 0x2214), ("plusdu", 0x2A25),
    ("pluse", 0x2A72), ("PlusMinus", 0xB1), ("plussim", 0x2A26), ("plustwo", 0x2A27), ("pm", 0xB1),
    ("Poincareplane", 0x210C), ("pointint", 0x2A15), ("Popf", 0x2119), ("popf", 0x1D561), ("Pr", 0x2ABB),
    ("pr", 0x227A), ("prap", 0x2AB7), ("prcue", 0x227C), ("prE", 0x2AB3), ("pre", 0x2AAF), ("prec", 0x227A),
    ("precapprox", 0x2AB7), ("preccurlyeq", 0x227C), ("Precedes", 0x227A), ("PrecedesEqual", 0x2AAF),
    ("PrecedesSlantEqual", 0x227C), ("PrecedesTilde", 0x227E), ("preceq", 0x2AAF), ("precnapprox", 0x2AB9),
    ("precneqq", 0x2AB5), ("precnsim", 0x22E8), ("precsim", 0x227E), ("primes", 0x2119), ("prnap", 0x2AB9),
    ("prnE", 0x2AB5), ("prnsim", 0x22E8), ("Product", 0x220F), ("profalar", 0x232E), ("profline", 0x2312),
    ("profsurf", 0x2313), ("Proportion", 0x2237), ("Proportional", 0x221D), ("propto", 0x221D), ("prsim", 0x227E),
    ("prurel", 0x22B0), ("Pscr", 0x1D4AB), ("pscr", 0x1D4C5), ("puncsp", 0x2008), ("Qfr", 0x1D514), ("qfr", 0x1D52E),
    ("qint", 0x2A0C), ("Qopf", 0x211A), ("qopf", 0x1D562), ("qprime", 0x2057), ("Qscr", 0x1D4AC), ("qscr", 0x1D4C6),
    ("quaternions", 0x210D), ("quatint", 0x2A16), ("quest", 0x3F), ("questeq", 0x225F), ("QUOT", 0x22),
    ("rAarr", 0x21DB), ("Racute", 0x154), ("racute", 0x155), ("raemptyv", 0x29B3), ("Rang", 0x27EB),
    ("rangd", 0x2992), ("range", 0x29A5), ("rangle", 0x27E9), ("Rarr", 0x21A0), ("rarrap", 0x2975), ("rarrb", 0x21E5),
    ("rarrbfs", 0x2920), ("rarrc", 0x2933), ("rarrfs", 0x291E), ("rarrhk", 0x21AA), ("rarrlp", 0x21AC),
    ("rarrpl", 0x2945), ("rarrsim", 0x2974), ("Rarrtl", 0x2916), ("rarrtl", 0x21A3), ("rarrw", 0x219D),
    ("rAtail", 0x291C), ("ratail", 0x291A), ("ratio", 0x2236), ("rationals", 0x211A), ("RBarr", 0x2910),
    ("rBarr", 0x290F), ("rbarr", 0x290D), ("rbbrk", 0x2773), ("rbrace", 0x7D), ("rbrack", 0x5D), ("rbrke", 0x298C),
    ("rbrksld", 0x298E), ("rbrkslu", 0x2990), ("Rcaron", 0x158), ("rcaron", 0x159), ("Rcedil", 0x156),
    ("rcedil", 0x157), ("rcub", 0x7D), ("Rcy", 0x420), ("rcy", 0x440), ("rdca", 0x2937), ("rdldhar", 0x2969),
    ("rdquor", 0x201D), ("rdsh", 0x21B3), ("Re", 0x211C), ("realine", 0x211B), ("realpart", 0x211C),
    ("reals", 0x211D), ("rect", 0x25AD), ("REG", 0xAE), ("ReverseElement", 0x220B), ("ReverseEquilibrium", 0x21CB),
    ("ReverseUpEquilibrium", 0x296F), ("rfisht", 0x297D), ("Rfr", 0x211C), ("rfr", 0x1D52F), ("rHar", 0x2964),
    ("rhard", 0x21C1), ("rharu", 0x21C0), ("rharul", 0x296C), ("rhov", 0x3F1), ("RightAngleBracket", 0x27E9),
    ("RightArrow", 0x2192), ("Rightarrow", 0x21D2), ("rightarrow", 0x2192), ("RightArrowBar", 0x21E5),
    ("RightArrowLeftArrow", 0x21C4), ("rightarrowtail", 0x21A3), ("RightCeiling", 0x2309),
    ("RightDoubleBracket", 0x27E7), ("RightDownTeeVector", 0x295D), ("RightDownVector", 0x21C2),
    ("RightDownVectorBar", 0x2955), ("RightFloor", 0x230B), ("rightharpoondown", 0x21C1), ("rightharpoonup", 0x21C0),
    ("rightleftarrows", 0x21C4), ("rightleftharpoons", 0x21CC), ("rightrightarrows", 0x21C9),
    ("rightsquigarrow", 0x219D), ("RightTee", 0x22A2), ("RightTeeArrow", 0x21A6), ("RightTeeVector", 0x295B),
    ("rightthreetimes", 0x22CC), ("RightTriangle", 0x22B3), ("RightTriangleBar", 0x29D0),
    ("RightTriangleEqual", 0x22B5), ("RightUpDownVector", 0x294F), ("RightUpTeeVector", 0x295C),
    ("RightUpVector", 0x21BE), ("RightUpVectorBar", 0x2954), ("RightVector", 0x21C0), ("RightVectorBar", 0x2953),
    ("ring", 0x2DA), ("risingdotseq", 0x2253), ("rlarr", 0x21C4), ("rlhar", 0x21CC), ("rmoust", 0x23B1),
    ("rmoustache", 0x23B1), ("rnmid", 0x2AEE), ("roang", 0x27ED), ("roarr", 0x21FE), ("robrk", 0x27E7),
    ("ropar", 0x2986), ("Ropf", 0x211D), ("ropf", 0x1D563), ("roplus", 0x2A2E), ("rotimes", 0x2A35),
    ("RoundImplies", 0x2970), ("rpar", 0x29), ("rpargt", 0x2994), ("rppolint", 0x2A12), ("rrarr", 0x21C9),
    ("Rrightarrow", 0x21DB), ("Rscr", 0x211B), ("rscr", 0x1D4C7), ("Rsh", 0x21B1), ("rsh", 0x21B1), ("rsqb", 0x5D),
    ("rsquor", 0x2019), ("rthree", 0x22CC), ("rtimes", 0x22CA), ("rtri", 0x25B9), ("rtrie", 0x22B5),
    ("rtrif", 0x25B8), ("rtriltri", 0x29CE), ("RuleDelayed", 0x29F4), ("ruluhar", 0x2968), ("rx", 0x211E),
    ("Sacute", 0x15A), ("sacute", 0x15B), ("Sc", 0x2ABC), ("sc", 0x227B), ("scap", 0x2AB8), ("sccue", 0x227D),
    ("scE", 0x2AB4), ("sce", 0x2AB0), ("Scedil", 0x15E), ("scedil", 0x15F), ("Scirc", 0x15C), ("scirc", 0x15D),
    ("scnap", 0x2ABA), ("scnE", 0x2AB6), ("scnsim", 0x22E9), ("scpolint", 0x2A13), ("scsim", 0x227F), ("Scy", 0x421),
    ("scy", 0x441), ("sdotb", 0x22A1), ("sdote", 0x2A66), ("searhk", 0x2925), ("seArr", 0x21D8), ("searr", 0x2198),
    ("searrow", 0x2198), ("semi", 0x3B), ("seswar", 0x2929), ("setminus", 0x2216), ("setmn", 0x2216),
    ("sext", 0x2736), ("Sfr", 0x1D516), ("sfr", 0x1D530), ("sfrown", 0x2322), ("sharp", 0x266F), ("SHCHcy", 0x429),
    ("shchcy", 0x449), ("SHcy", 0x428), ("shcy", 0x448), ("ShortDownArrow", 0x2193), ("ShortLeftArrow", 0x2190),
    ("shortmid", 0x2223), ("shortparallel", 0x2225), ("ShortRightArrow", 0x2192), ("ShortUpArrow", 0x2191),
    ("sigmav", 0x3C2), ("simdot", 0x2A6A), ("sime", 0x2243), ("simeq", 0x2243), ("simg", 0x2A9E), ("simgE", 0x2AA0),
    ("siml", 0x2A9D), ("simlE", 0x2A9F), ("simne", 0x2246), ("simplus", 0x2A24), ("simrarr", 0x2972),
    ("slarr", 0x2190), ("SmallCircle", 0x2218), ("smallsetminus", 0x2216), ("smashp", 0x2A33), ("smeparsl", 0x29E4),
    ("smid", 0x2223), ("smile", 0x2323), ("smt", 0x2AAA), ("smte", 0x2AAC), ("SOFTcy", 0x42C), ("softcy", 0x44C),
    ("sol", 0x2F), ("solb", 0x29C4), ("solbar", 0x233F), ("Sopf", 0x1D54A), ("sopf", 0x1D564), ("spadesuit", 0x2660),
    ("spar", 0x2225), ("sqcap", 0x2293), ("sqcup", 0x2294), ("Sqrt", 0x221A), ("sqsub", 0x228F), ("sqsube", 0x2291),
    ("sqsubset", 0x228F), ("sqsubseteq", 0x2291), ("sqsup", 0x2290), ("sqsupe", 0x2292), ("sqsupset", 0x2290),
    ("sqsupseteq", 0x2292), ("squ", 0x25A1), ("Square", 0x25A1), ("square", 0x25A1), ("SquareIntersection", 0x2293),
    ("SquareSubset", 0x228F), ("SquareSubsetEqual", 0x2291), ("SquareSuperset", 0x2290),
    ("SquareSupersetEqual", 0x2292), ("SquareUnion", 0x2294), ("squarf", 0x25AA), ("squf", 0x25AA), ("srarr", 0x2192),
    ("Sscr", 0x1D4AE), ("sscr", 0x1D4C8), ("ssetmn", 0x2216), ("ssmile", 0x2323), ("sstarf", 0x22C6),
    ("Star", 0x22C6), ("star", 0x2606), ("starf", 0x2605), ("straightepsilon", 0x3F5), ("straightphi", 0x3D5),
    ("strns", 0xAF), ("Sub", 0x22D0), ("subdot", 0x2ABD), ("subE", 0x2AC5), ("subedot", 0x2AC3), ("submult", 0x2AC1),
    ("subnE", 0x2ACB), ("subne", 0x228A), ("subplus", 0x2ABF), ("subrarr", 0x2979), ("Subset", 0x22D0),
    ("subset", 0x2282), ("subseteq", 0x2286), ("subseteqq", 0x2AC5), ("SubsetEqual", 0x2286), ("subsetneq", 0x228A),
    ("subsetneqq", 0x2ACB), ("subsim", 0x2AC7), ("subsub", 0x2AD5), ("subsup", 0x2AD3), ("succ", 0x227B),
    ("succapprox", 0x2AB8), ("succcurlyeq", 0x227D), ("Succeeds", 0x227B), ("SucceedsEqual", 0x2AB0),
    ("SucceedsSlantEqual", 0x227D), ("SucceedsTilde", 0x227F), ("succeq", 0x2AB0), ("succnapprox", 0x2ABA),
    ("succneqq", 0x2AB6), ("succnsim", 0x22E9), ("succsim", 0x227F), ("SuchThat", 0x220B), ("Sum", 0x2211),
    ("sung", 0x266A), ("Sup", 0x22D1), ("supdot", 0x2ABE), ("supdsub", 0x2AD8), ("supE", 0x2AC6), ("supedot", 0x2AC4),
    ("Superset", 0x2283), ("SupersetEqual", 0x2287), ("suphsol", 0x27C9), ("suphsub", 0x2AD7), ("suplarr", 0x297B),
    ("supmult", 0x2AC2), ("supnE", 0x2ACC), ("supne", 0x228B), ("supplus", 0x2AC0), ("Supset", 0x22D1),
    ("supset", 0x2283), ("supseteq", 0x2287), ("supseteqq", 0x2AC6), ("supsetneq", 0x228B), ("supsetneqq", 0x2ACC),
    ("supsim", 0x2AC8), ("supsub", 0x2AD4), ("supsup", 0x2AD6), ("swarhk", 0x2926), ("swArr", 0x21D9),
    ("swarr", 0x2199), ("swarrow", 0x2199), ("swnwar", 0x292A), ("Tab", 0x9), ("target", 0x2316), ("tbrk", 0x23B4),
    ("Tcaron", 0x164), ("tcaron", 0x165), ("Tcedil", 0x162), ("tcedil", 0x163), ("Tcy", 0x422), ("tcy", 0x442),
    ("tdot", 0x20DB), ("telrec", 0x2315), ("Tfr", 0x1D517), ("tfr", 0x1D531), ("Therefore", 0x2234),
    ("therefore", 0x2234), ("thetav", 0x3D1), ("thickapprox", 0x2248), ("thicksim", 0x223C), ("ThinSpace", 0x2009),
    ("thkap", 0x2248), ("thksim", 0x223C), ("Tilde", 0x223C), ("TildeEqual", 0x2243), ("TildeFullEqual", 0x2245),
    ("TildeTilde", 0x2248), ("timesb", 0x22A0), ("timesbar", 0x2A31), ("timesd", 0x2A30), ("tint", 0x222D),
    ("toea", 0x2928), ("top", 0x22A4), ("topbot", 0x2336), ("topcir", 0x2AF1), ("Topf", 0x1D54B), ("topf", 0x1D565),
    ("topfork", 0x2ADA), ("tosa", 0x2929), ("tprime", 0x2034), ("TRADE", 0x2122), ("triangle", 0x25B5),
    ("triangledown", 0x25BF), ("triangleleft", 0x25C3), ("trianglelefteq", 0x22B4), ("triangleq", 0x225C),
    ("triangleright", 0x25B9), ("trianglerighteq", 0x22B5), ("tridot", 0x25EC), ("trie", 0x225C),
    ("triminus", 0x2A3A), ("TripleDot", 0x20DB), ("triplus", 0x2A39), ("trisb", 0x29CD), ("tritime", 0x2A3B),
    ("trpezium", 0x23E2), ("Tscr", 0x1D4AF), ("tscr", 0x1D4C9), ("TScy", 0x426), ("tscy", 0x446), ("TSHcy", 0x40B),
    ("tshcy", 0x45B), ("Tstrok", 0x166), ("tstrok", 0x167), ("twixt", 0x226C), ("twoheadleftarrow", 0x219E),
    ("twoheadrightarrow", 0x21A0), ("Uarr", 0x219F), ("Uarrocir", 0x2949), ("Ubrcy", 0x40E), ("ubrcy", 0x45E),
    ("Ubreve", 0x16C), ("ubreve", 0x16D), ("Ucy", 0x423), ("ucy", 0x443), ("udarr", 0x21C5), ("Udblac", 0x170),
    ("udblac", 0x171), ("udhar", 0x296E), ("ufisht", 0x297E), ("Ufr", 0x1D518), ("ufr", 0x1D532), ("uHar", 0x2963),
    ("uharl", 0x21BF), ("uharr", 0x21BE), ("uhblk", 0x2580), ("ulcorn", 0x231C), ("ulcorner", 0x231C),
    ("ulcrop", 0x230F), ("ultri", 0x25F8), ("Umacr", 0x16A), ("umacr", 0x16B), ("UnderBar", 0x5F),
    ("UnderBrace", 0x23DF), ("UnderBracket", 0x23B5), ("UnderParenthesis", 0x23DD), ("Union", 0x22C3),
    ("UnionPlus", 0x228E), ("Uogon", 0x172), ("uogon", 0x173), ("Uopf", 0x1D54C), ("uopf", 0x1D566),
    ("UpArrow", 0x2191), ("Uparrow", 0x21D1), ("uparrow", 0x2191), ("UpArrowBar", 0x2912),
    ("UpArrowDownArrow", 0x21C5), ("UpDownArrow", 0x2195), ("Updownarrow", 0x21D5), ("updownarrow", 0x2195),
    ("UpEquilibrium", 0x296E), ("upharpoonleft", 0x21BF), ("upharpoonright", 0x21BE), ("uplus", 0x228E),
    ("UpperLeftArrow", 0x2196), ("UpperRightArrow", 0x2197), ("Upsi", 0x3D2), ("upsi", 0x3C5), ("UpTee", 0x22A5),
    ("UpTeeArrow", 0x21A5), ("upuparrows", 0x21C8), ("urcorn", 0x231D), ("urcorner", 0x231D), ("urcrop", 0x230E),
    ("Uring", 0x16E), ("uring", 0x16F), ("urtri", 0x25F9), ("Uscr", 0x1D4B0), ("uscr", 0x1D4CA), ("utdot", 0x22F0),
    ("Utilde", 0x168), ("utilde", 0x169), ("utri", 0x25B5), ("utrif", 0x25B4), ("uuarr", 0x21C8), ("uwangle", 0x29A7),
    ("vangrt", 0x299C), ("varepsilon", 0x3F5), ("varkappa", 0x3F0), ("varnothing", 0x2205), ("varphi", 0x3D5),
    ("varpi", 0x3D6), ("varpropto", 0x221D), ("vArr", 0x21D5), ("varr", 0x2195), ("varrho", 0x3F1),
    ("varsigma", 0x3C2), ("vartheta", 0x3D1), ("vartriangleleft", 0x22B2), ("vartriangleright", 0x22B3),
    ("Vbar", 0x2AEB), ("vBar", 0x2AE8), ("vBarv", 0x2AE9), ("Vcy", 0x412), ("vcy", 0x432), ("VDash", 0x22AB),
    ("Vdash", 0x22A9), ("vDash", 0x22A8), ("vdash", 0x22A2), ("Vdashl", 0x2AE6), ("Vee", 0x22C1), ("vee", 0x2228),
    ("veebar", 0x22BB), ("veeeq", 0x225A), ("vellip", 0x22EE), ("Verbar", 0x2016), ("verbar", 0x7C), ("Vert", 0x2016),
    ("vert", 0x7C), ("VerticalBar", 0x2223), ("VerticalLine", 0x7C), ("VerticalSeparator", 0x2758),
    ("VerticalTilde", 0x2240), ("VeryThinSpace", 0x200A), ("Vfr", 0x1D519), ("vfr", 0x1D533), ("vltri", 0x22B2),
    ("Vopf", 0x1D54D), ("vopf", 0x1D567), ("vprop", 0x221D), ("vrtri", 0x22B3), ("Vscr", 0x1D4B1), ("vscr", 0x1D4CB),
    ("Vvdash", 0x22AA), ("vzigzag", 0x299A), ("Wcirc", 0x174), ("wcirc", 0x175), ("wedbar", 0x2A5F),
    ("Wedge", 0x22C0), ("wedge", 0x2227), ("wedgeq", 0x2259), ("Wfr", 0x1D51A), ("wfr", 0x1D534), ("Wopf", 0x1D54E),
    ("wopf", 0x1D568), ("wp", 0x2118), ("wr", 0x2240), ("wreath", 0x2240), ("Wscr", 0x1D4B2), ("wscr", 0x1D4CC),
    ("xcap", 0x22C2), ("xcirc", 0x25EF), ("xcup", 0x22C3), ("xdtri", 0x25BD), ("Xfr", 0x1D51B), ("xfr", 0x1D535),
    ("xhArr", 0x27FA), ("xharr", 0x27F7), ("xlArr", 0x27F8), ("xlarr", 0x27F5), ("xmap", 0x27FC), ("xnis", 0x22FB),
    ("xodot", 0x2A00), ("Xopf", 0x1D54F), ("xopf", 0x1D569), ("xoplus", 0x2A01), ("xotime", 0x2A02),
    ("xrArr", 0x27F9), ("xrarr", 0x27F6), ("Xscr", 0x1D4B3), ("xscr", 0x1D4CD), ("xsqcup", 0x2A06),
    ("xuplus", 0x2A04), ("xutri", 0x25B3), ("xvee", 0x22C1), ("xwedge", 0x22C0), ("YAcy", 0x42F), ("yacy", 0x44F),
    ("Ycirc", 0x176), ("ycirc", 0x177), ("Ycy", 0x42B), ("ycy", 0x44B), ("Yfr", 0x1D51C), ("yfr", 0x1D536),
    ("YIcy", 0x407), ("yicy", 0x457), ("Yopf", 0x1D550), ("yopf", 0x1D56A), ("Yscr", 0x1D4B4), ("yscr", 0x1D4CE),
    ("YUcy", 0x42E), ("yucy", 0x44E), ("Zacute", 0x179), ("zacute", 0x17A), ("Zcaron", 0x17D), ("zcaron", 0x17E),
    ("Zcy", 0x417), ("zcy", 0x437), ("Zdot", 0x17B), ("zdot", 0x17C), ("zeetrf", 0x2128), ("ZeroWidthSpace", 0x200B),
    ("Zfr", 0x2128), ("zfr", 0x1D537), ("ZHcy", 0x416), ("zhcy", 0x436), ("zigrarr", 0x21DD), ("Zopf", 0x2124),
    ("zopf", 0x1D56B), ("Zscr", 0x1D4B5), ("zscr", 0x1D4CF),
];

/// The longest name in the tables (`CounterClockwiseContourIntegral`).
const MAX_NAME: usize = 31;

/// The longest name read without a semicolon (`frac34` and others).
const MAX_LEGACY: usize = 6;

fn table() -> &'static HashMap<&'static str, char> {
    static TABLE: OnceLock<HashMap<&'static str, char>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::with_capacity(LATIN1.len() + NAMED.len() + HTML5.len());
        for (i, name) in LATIN1.iter().enumerate() {
            map.insert(*name, char::from_u32(0xA0 + i as u32).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
        for &(name, code) in NAMED.iter().chain(HTML5) {
            map.insert(name, char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
        map
    })
}

/// The character a name stands for, e.g. `mdash`.
pub(crate) fn lookup(name: &str) -> Option<char> {
    table().get(name).copied()
}

fn is_legacy(name: &str) -> bool {
    LATIN1.contains(&name) || LEGACY.contains(&name)
}

/// A numeric reference's code point as a character: zero, surrogates and
/// values beyond Unicode become U+FFFD.
fn from_code(code: u32, windows_1252: bool) -> char {
    if windows_1252 && (0x80..0xA0).contains(&code) {
        // HTML reads these as the Windows-1252 characters authors meant.
        let decoded = Encoding::WINDOWS_1252.decode(&[code as u8]).text;
        return decoded.chars().next().unwrap_or(char::REPLACEMENT_CHARACTER);
    }
    match code {
        0 => char::REPLACEMENT_CHARACTER,
        _ => char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER),
    }
}

/// `&#...;` or `&#x...;` at the start of `s`: the character and the bytes
/// it took. `strict` asks for CommonMark's rules (a semicolon, at most 7
/// decimal or 6 hex digits); otherwise HTML's.
fn numeric(s: &[u8], strict: bool) -> Option<(char, usize)> {
    let hex = matches!(s.get(2), Some(b'x' | b'X'));
    let start = if hex { 3 } else { 2 };
    let radix = if hex { 16 } else { 10 };
    let digits = s[start.min(s.len())..].iter().take_while(|c| (**c as char).is_digit(radix)).count();
    if digits == 0 || (strict && digits > if hex { 6 } else { 7 }) {
        return None;
    }
    let mut code: u32 = 0;
    for &c in &s[start..start + digits] {
        let digit = (c as char).to_digit(radix).unwrap_or(0);
        code = code.saturating_mul(radix).saturating_add(digit);
    }
    let mut end = start + digits;
    if s.get(end) == Some(&b';') {
        end += 1;
    } else if strict {
        return None;
    }
    Some((from_code(code, !strict), end))
}

/// A character reference at the start of `s` (which begins with `&`), read
/// the way HTML does: numeric references and names with or without their
/// semicolon. In attribute values a legacy name followed by a letter, digit
/// or `=` is left alone, so `?a=1&copy=2` keeps its `&copy`.
pub(crate) fn decode_html(s: &str, in_attribute: bool) -> Option<(char, usize)> {
    let b = s.as_bytes();
    if b.get(1) == Some(&b'#') {
        return numeric(b, false);
    }
    let len = b[1..].iter().take_while(|c| c.is_ascii_alphanumeric()).take(MAX_NAME + 1).count();
    if len == 0 {
        return None;
    }
    let name = &s[1..1 + len];
    if b.get(1 + len) == Some(&b';') {
        if let Some(c) = lookup(name) {
            return Some((c, len + 2));
        }
    }
    // Legacy names may stand without a semicolon, even before more letters.
    for end in (2..=len.min(MAX_LEGACY)).rev() {
        let prefix = &name[..end];
        if !is_legacy(prefix) {
            continue;
        }
        let next = b.get(1 + end).copied();
        if in_attribute && next != Some(b';') && next.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'=') {
            return None;
        }
        let used = if next == Some(b';') { end + 2 } else { end + 1 };
        return lookup(prefix).map(|c| (c, used));
    }
    None
}

/// A character reference at the start of `s` (which begins with `&`), read
/// the way CommonMark does: a known name or a number, always followed by a
/// semicolon.
pub(crate) fn decode_strict(s: &str) -> Option<(char, usize)> {
    let b = s.as_bytes();
    if b.get(1) == Some(&b'#') {
        return numeric(b, true);
    }
    if !b.get(1).is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let len = b[1..].iter().take_while(|c| c.is_ascii_alphanumeric()).take(MAX_NAME + 1).count();
    if len > MAX_NAME || b.get(1 + len) != Some(&b';') {
        return None;
    }
    lookup(&s[1..1 + len]).map(|c| (c, len + 2))
}

/// Text with every HTML character reference decoded.
pub(crate) fn decode_all(text: &str, in_attribute: bool) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match decode_html(rest, in_attribute) {
            Some((c, used)) => {
                out.push(c);
                rest = &rest[used..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_every_html4_entity() {
        // 96 Latin-1, 32 special and 124 symbol names, plus apos; in all,
        // HTML5's 2125 names less the 93 for two characters.
        assert_eq!(LATIN1.len() + NAMED.len(), 253);
        assert_eq!(table().len(), 2125 - 93);
        assert_eq!(lookup("nbsp"), Some('\u{A0}'));
        assert_eq!(lookup("yuml"), Some('ÿ'));
        assert_eq!(lookup("hearts"), Some('♥'));
        assert_eq!(lookup("Omega"), Some('Ω'));
        assert_eq!(lookup("apos"), Some('\''));
        assert_eq!(lookup("nosuch"), None);
        assert_eq!(lookup("frac13"), Some('⅓'));
        assert_eq!(lookup("CounterClockwiseContourIntegral"), Some('∳'));
        assert_eq!(lookup("Dcaron"), Some('Ď'));
        assert_eq!(lookup("ngE"), None);
        let names = LATIN1.iter().chain(NAMED.iter().chain(HTML5).map(|(n, _)| n));
        assert_eq!(names.map(|n| n.len()).max(), Some(MAX_NAME));
        assert_eq!(LATIN1.iter().chain(&LEGACY).map(|n| n.len()).max(), Some(MAX_LEGACY));
    }

    #[test]
    fn decodes_like_html() {
        assert_eq!(decode_all("a &amp; b &lt;c&gt; &copy 2024 &mdash; &#8212; &#x2014; &#X41;", false), "a & b <c> © 2024 — — — A");
        assert_eq!(decode_all("&notit; &notin; &bogus; & x", false), "¬it; ∉ &bogus; & x");
        assert_eq!(decode_all("&#0; &#xD800; &#x110000; &#150; &#99999999999", false), "\u{FFFD} \u{FFFD} \u{FFFD} – \u{FFFD}");
        assert_eq!(decode_all("?a=1&copy=2&amp;b=3&lt", true), "?a=1&copy=2&b=3<");
        assert_eq!(decode_all("&ampx", false), "&x");
    }

    #[test]
    fn decodes_like_commonmark() {
        assert_eq!(decode_strict("&amp;x"), Some(('&', 5)));
        assert_eq!(decode_strict("&copy"), None);
        assert_eq!(decode_strict("&#35;"), Some(('#', 5)));
        assert_eq!(decode_strict("&#1234567;"), Some(('\u{FFFD}', 10)));
        assert_eq!(decode_strict("&#12345678;"), None);
        assert_eq!(decode_strict("&#xcab;"), Some(('\u{CAB}', 7)));
        assert_eq!(decode_strict("&#0;"), Some(('\u{FFFD}', 4)));
        assert_eq!(decode_strict("&#150;"), Some(('\u{96}', 6)));
        assert_eq!(decode_strict("&x;"), None);
    }
}
