//! What the analysis knows of HTML without the language server: the elements, their
//! attributes and the values these take, and the character entities.

/// The elements of HTML, with the attributes of their own (the global ones are in
/// [`GLOBAL_ATTRIBUTES`]). One per line: `name: attribute attribute…`.
const ELEMENTS: &str = "\
a: href target download rel hreflang type referrerpolicy ping
abbr:
address:
area: alt coords shape href target download rel referrerpolicy ping
article:
aside:
audio: src controls autoplay loop muted preload crossorigin
b:
base: href target
bdi:
bdo:
blockquote: cite
body:
br:
button: type name value disabled form formaction formenctype formmethod formnovalidate formtarget popovertarget popovertargetaction autofocus
canvas: width height
caption:
cite:
code:
col: span
colgroup: span
data: value
datalist:
dd:
del: cite datetime
details: open name
dfn:
dialog: open
div:
dl:
dt:
em:
embed: src type width height
fieldset: disabled form name
figcaption:
figure:
footer:
form: action method enctype target novalidate autocomplete name accept-charset rel
h1:
h2:
h3:
h4:
h5:
h6:
head:
header:
hgroup:
hr:
html: lang xmlns manifest
i:
iframe: src srcdoc name width height allow allowfullscreen loading referrerpolicy sandbox
img: src alt width height srcset sizes loading decoding crossorigin referrerpolicy usemap ismap fetchpriority
input: type name value placeholder required disabled readonly checked autocomplete autofocus min max step minlength maxlength pattern size multiple accept list form formaction formenctype formmethod formnovalidate formtarget src alt width height dirname capture
ins: cite datetime
kbd:
label: for form
legend:
li: value
link: href rel type media sizes as crossorigin integrity hreflang referrerpolicy title disabled fetchpriority
main:
map: name
mark:
menu:
meta: name content charset http-equiv property media
meter: value min max low high optimum form
nav:
noscript:
object: data type name width height form usemap
ol: reversed start type
optgroup: label disabled
option: value label selected disabled
output: for form name
p:
param: name value
picture:
pre:
progress: value max
q: cite
rp:
rt:
ruby:
s:
samp:
search:
script: src type async defer crossorigin integrity nomodule referrerpolicy blocking fetchpriority
section:
select: name required disabled multiple size autocomplete autofocus form
slot: name
small:
source: src srcset sizes type media width height
span:
strong:
style: media blocking
sub:
summary:
sup:
table:
tbody:
td: colspan rowspan headers
template: shadowrootmode
textarea: name placeholder rows cols required disabled readonly maxlength minlength wrap autocomplete autofocus form dirname spellcheck
tfoot:
th: colspan rowspan headers scope abbr
thead:
time: datetime
title:
tr:
track: src kind srclang label default
u:
ul:
var:
video: src controls autoplay loop muted poster preload width height playsinline crossorigin
wbr:
svg: width height viewBox xmlns fill stroke preserveAspectRatio
math: display xmlns";

/// The attributes every element takes.
pub const GLOBAL_ATTRIBUTES: &[&str] = &[
    "id",
    "class",
    "style",
    "title",
    "lang",
    "dir",
    "hidden",
    "tabindex",
    "accesskey",
    "contenteditable",
    "draggable",
    "spellcheck",
    "translate",
    "inert",
    "inputmode",
    "enterkeyhint",
    "autocapitalize",
    "autofocus",
    "popover",
    "is",
    "nonce",
    "part",
    "slot",
    "role",
    "itemscope",
    "itemprop",
    "itemtype",
    "itemid",
    "itemref",
];

/// The most used ARIA attributes, offered on every element.
pub const ARIA_ATTRIBUTES: &[&str] = &[
    "aria-label",
    "aria-labelledby",
    "aria-describedby",
    "aria-hidden",
    "aria-expanded",
    "aria-controls",
    "aria-current",
    "aria-disabled",
    "aria-live",
    "aria-haspopup",
    "aria-pressed",
    "aria-selected",
    "aria-checked",
    "aria-modal",
    "aria-owns",
    "aria-required",
    "aria-invalid",
    "aria-busy",
];

/// The event handlers, offered on every element.
pub const EVENT_ATTRIBUTES: &[&str] = &[
    "onclick",
    "ondblclick",
    "onchange",
    "oninput",
    "onsubmit",
    "onreset",
    "onfocus",
    "onblur",
    "onkeydown",
    "onkeyup",
    "onkeypress",
    "onmousedown",
    "onmouseup",
    "onmouseover",
    "onmouseout",
    "onmouseenter",
    "onmouseleave",
    "onmousemove",
    "onpointerdown",
    "onpointerup",
    "onpointermove",
    "ontouchstart",
    "ontouchend",
    "onwheel",
    "onscroll",
    "onload",
    "onerror",
    "onresize",
    "oncontextmenu",
    "ondrag",
    "ondragstart",
    "ondragend",
    "ondragover",
    "ondrop",
    "oncopy",
    "oncut",
    "onpaste",
    "onplay",
    "onpause",
    "onended",
    "ontoggle",
    "onanimationend",
    "ontransitionend",
];

/// The attributes that take no value: present, they are true.
pub const BOOLEAN_ATTRIBUTES: &[&str] = &[
    "allowfullscreen",
    "async",
    "autofocus",
    "autoplay",
    "checked",
    "controls",
    "default",
    "defer",
    "disabled",
    "formnovalidate",
    "hidden",
    "inert",
    "ismap",
    "itemscope",
    "loop",
    "multiple",
    "muted",
    "nomodule",
    "novalidate",
    "open",
    "playsinline",
    "readonly",
    "required",
    "reversed",
    "selected",
];

/// The elements that have no content and no closing tag.
pub fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
            | "keygen"
    )
}

/// The elements whose closing tag may be left out.
pub fn optional_end(name: &str) -> bool {
    matches!(
        name,
        "html"
            | "head"
            | "body"
            | "p"
            | "li"
            | "dt"
            | "dd"
            | "option"
            | "optgroup"
            | "tr"
            | "td"
            | "th"
            | "thead"
            | "tbody"
            | "tfoot"
            | "colgroup"
            | "caption"
            | "rt"
            | "rp"
    )
}

/// Whether opening `new` closes an open `current` whose closing tag was left out (a `<li>`
/// after a `<li>`, a `<div>` after a `<p>`).
pub fn closes(current: &str, new: &str) -> bool {
    match current {
        "p" => matches!(
            new,
            "address"
                | "article"
                | "aside"
                | "blockquote"
                | "details"
                | "dialog"
                | "div"
                | "dl"
                | "fieldset"
                | "figcaption"
                | "figure"
                | "footer"
                | "form"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "header"
                | "hgroup"
                | "hr"
                | "main"
                | "menu"
                | "nav"
                | "ol"
                | "p"
                | "pre"
                | "search"
                | "section"
                | "table"
                | "ul"
        ),
        "li" => new == "li",
        "dt" | "dd" => matches!(new, "dt" | "dd"),
        "option" => matches!(new, "option" | "optgroup"),
        "optgroup" => new == "optgroup",
        "tr" => matches!(new, "tr" | "tbody" | "tfoot" | "thead"),
        "td" | "th" => matches!(new, "td" | "th" | "tr" | "tbody" | "tfoot" | "thead"),
        "thead" | "tbody" => matches!(new, "tbody" | "tfoot"),
        "rt" | "rp" => matches!(new, "rt" | "rp"),
        "colgroup" | "caption" => !matches!(new, "col"),
        "head" => new == "body",
        _ => false,
    }
}

/// Every element, in the order of the list.
pub fn elements() -> impl Iterator<Item = &'static str> {
    ELEMENTS
        .lines()
        .filter_map(|l| l.split_once(':').map(|(name, _)| name))
}

pub fn is_element(name: &str) -> bool {
    elements().any(|e| e == name)
}

/// The attributes of an element of its own, without the global ones.
pub fn own_attributes(element: &str) -> Vec<&'static str> {
    ELEMENTS
        .lines()
        .find_map(|l| {
            let (name, attributes) = l.split_once(':')?;
            (name == element).then(|| attributes.split_whitespace().collect())
        })
        .unwrap_or_default()
}

/// The values an attribute takes, when it takes a few known ones.
pub fn values(element: &str, attribute: &str) -> &'static [&'static str] {
    match (element, attribute) {
        ("input", "type") => &[
            "text",
            "password",
            "email",
            "number",
            "tel",
            "url",
            "search",
            "date",
            "time",
            "datetime-local",
            "month",
            "week",
            "color",
            "range",
            "checkbox",
            "radio",
            "file",
            "hidden",
            "submit",
            "reset",
            "button",
            "image",
        ],
        ("button", "type") => &["button", "submit", "reset"],
        ("script", "type") => &["module", "text/javascript", "importmap", "application/json"],
        ("style" | "link", "type") => &["text/css"],
        ("ol", "type") => &["1", "a", "A", "i", "I"],
        ("link", "rel") => &[
            "stylesheet",
            "icon",
            "preload",
            "prefetch",
            "preconnect",
            "dns-prefetch",
            "modulepreload",
            "manifest",
            "canonical",
            "alternate",
            "apple-touch-icon",
            "author",
            "license",
        ],
        ("a" | "area" | "form", "rel") => &[
            "noopener",
            "noreferrer",
            "nofollow",
            "external",
            "help",
            "license",
            "next",
            "prev",
            "author",
            "bookmark",
            "tag",
        ],
        ("link", "as") => &[
            "script", "style", "font", "image", "fetch", "document", "audio", "video", "track",
            "worker",
        ],
        (_, "target" | "formtarget") => &["_blank", "_self", "_parent", "_top"],
        ("form", "method") | (_, "formmethod") => &["get", "post", "dialog"],
        ("form", "enctype") | (_, "formenctype") => &[
            "application/x-www-form-urlencoded",
            "multipart/form-data",
            "text/plain",
        ],
        (_, "dir") => &["ltr", "rtl", "auto"],
        (_, "loading") => &["lazy", "eager"],
        (_, "decoding") => &["async", "sync", "auto"],
        (_, "fetchpriority") => &["high", "low", "auto"],
        (_, "crossorigin") => &["anonymous", "use-credentials"],
        (_, "referrerpolicy") => &[
            "no-referrer",
            "no-referrer-when-downgrade",
            "origin",
            "origin-when-cross-origin",
            "same-origin",
            "strict-origin",
            "strict-origin-when-cross-origin",
            "unsafe-url",
        ],
        (_, "preload") => &["none", "metadata", "auto"],
        (_, "wrap") => &["soft", "hard"],
        (_, "kind") => &[
            "subtitles",
            "captions",
            "descriptions",
            "chapters",
            "metadata",
        ],
        (_, "scope") => &["row", "col", "rowgroup", "colgroup"],
        (_, "shape") => &["rect", "circle", "poly", "default"],
        (_, "sandbox") => &[
            "allow-forms",
            "allow-modals",
            "allow-popups",
            "allow-same-origin",
            "allow-scripts",
            "allow-downloads",
            "allow-top-navigation",
        ],
        ("meta", "name") => &[
            "viewport",
            "description",
            "keywords",
            "author",
            "robots",
            "theme-color",
            "color-scheme",
            "generator",
            "referrer",
        ],
        ("meta", "http-equiv") => &[
            "content-type",
            "default-style",
            "refresh",
            "x-ua-compatible",
            "content-security-policy",
        ],
        (_, "charset") => &["UTF-8"],
        (_, "autocomplete") => &[
            "on",
            "off",
            "name",
            "email",
            "username",
            "new-password",
            "current-password",
            "one-time-code",
            "tel",
            "street-address",
            "postal-code",
            "country",
            "bday",
        ],
        (_, "inputmode") => &[
            "none", "text", "decimal", "numeric", "tel", "search", "email", "url",
        ],
        (_, "enterkeyhint") => &["enter", "done", "go", "next", "previous", "search", "send"],
        (_, "autocapitalize") => &["off", "none", "on", "sentences", "words", "characters"],
        (_, "contenteditable") => &["true", "false", "plaintext-only"],
        (_, "draggable" | "spellcheck") => &["true", "false"],
        (_, "translate") => &["yes", "no"],
        (_, "popover") => &["auto", "manual"],
        (_, "popovertargetaction") => &["toggle", "show", "hide"],
        (_, "shadowrootmode") => &["open", "closed"],
        (
            _,
            "aria-hidden" | "aria-expanded" | "aria-disabled" | "aria-modal" | "aria-required"
            | "aria-busy",
        ) => &["true", "false"],
        (_, "aria-pressed" | "aria-checked") => &["true", "false", "mixed"],
        (_, "aria-live") => &["off", "polite", "assertive"],
        (_, "aria-current") => &["page", "step", "location", "date", "time", "true", "false"],
        (_, "aria-haspopup") => &["true", "false", "menu", "listbox", "tree", "grid", "dialog"],
        (_, "aria-invalid") => &["true", "false", "grammar", "spelling"],
        (_, "role") => &[
            "button",
            "link",
            "navigation",
            "main",
            "banner",
            "contentinfo",
            "complementary",
            "dialog",
            "alert",
            "alertdialog",
            "menu",
            "menubar",
            "menuitem",
            "tab",
            "tablist",
            "tabpanel",
            "list",
            "listitem",
            "img",
            "presentation",
            "none",
            "search",
            "region",
            "status",
            "checkbox",
            "switch",
            "textbox",
            "tooltip",
        ],
        _ => &[],
    }
}

/// The attributes whose value names an `id` of the document.
pub fn refers_to_id(attribute: &str) -> bool {
    matches!(
        attribute,
        "for"
            | "list"
            | "form"
            | "headers"
            | "popovertarget"
            | "aria-labelledby"
            | "aria-describedby"
            | "aria-controls"
            | "aria-owns"
            | "itemref"
    )
}

/// The attributes whose value is a link that may point at an `id` (`#name`).
pub fn is_link(attribute: &str) -> bool {
    matches!(attribute, "href" | "action" | "formaction" | "usemap")
}

/// The most used named character references, as `name` and the character they stand for.
pub const ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", "\u{a0}"),
    ("copy", "©"),
    ("reg", "®"),
    ("trade", "™"),
    ("euro", "€"),
    ("pound", "£"),
    ("yen", "¥"),
    ("cent", "¢"),
    ("sect", "§"),
    ("para", "¶"),
    ("deg", "°"),
    ("plusmn", "±"),
    ("times", "×"),
    ("divide", "÷"),
    ("frac12", "½"),
    ("frac14", "¼"),
    ("frac34", "¾"),
    ("middot", "·"),
    ("bull", "•"),
    ("hellip", "…"),
    ("ndash", "–"),
    ("mdash", "—"),
    ("lsquo", "‘"),
    ("rsquo", "’"),
    ("ldquo", "“"),
    ("rdquo", "”"),
    ("laquo", "«"),
    ("raquo", "»"),
    ("larr", "←"),
    ("rarr", "→"),
    ("uarr", "↑"),
    ("darr", "↓"),
    ("harr", "↔"),
    ("check", "✓"),
    ("hearts", "♥"),
    ("infin", "∞"),
    ("ne", "≠"),
    ("le", "≤"),
    ("ge", "≥"),
    ("shy", "\u{ad}"),
    ("zwj", "\u{200d}"),
    ("ensp", "\u{2002}"),
    ("emsp", "\u{2003}"),
    ("thinsp", "\u{2009}"),
];
