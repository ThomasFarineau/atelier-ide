//! What the completion knows of CSS without the language server: the properties and their
//! values, the pseudo-classes, the at-rules and the functions.

/// `property: value value…` per line; the values every property takes are in [`GLOBAL_VALUES`].
const PROPERTIES: &str = "\
accent-color: auto
align-content: normal start end center flex-start flex-end space-between space-around space-evenly stretch baseline
align-items: normal stretch center start end flex-start flex-end baseline self-start self-end
align-self: auto normal stretch center start end flex-start flex-end baseline self-start self-end
all:
animation: none
animation-delay:
animation-direction: normal reverse alternate alternate-reverse
animation-duration:
animation-fill-mode: none forwards backwards both
animation-iteration-count: infinite
animation-name: none
animation-play-state: running paused
animation-timing-function: ease ease-in ease-out ease-in-out linear step-start step-end
appearance: none auto
aspect-ratio: auto
backdrop-filter: none
backface-visibility: visible hidden
background: none transparent
background-attachment: scroll fixed local
background-blend-mode: normal multiply screen overlay darken lighten color-dodge color-burn hard-light soft-light difference exclusion hue saturation color luminosity
background-clip: border-box padding-box content-box text
background-color: transparent currentColor
background-image: none
background-origin: border-box padding-box content-box
background-position: top bottom left right center
background-repeat: repeat no-repeat repeat-x repeat-y space round
background-size: auto cover contain
block-size: auto fit-content min-content max-content
border: none solid dashed dotted double groove ridge inset outset hidden
border-block: none solid
border-bottom: none solid dashed dotted
border-bottom-color: currentColor transparent
border-bottom-left-radius:
border-bottom-right-radius:
border-bottom-style: none solid dashed dotted double groove ridge inset outset hidden
border-bottom-width: thin medium thick
border-collapse: collapse separate
border-color: currentColor transparent
border-image: none
border-inline: none solid
border-left: none solid dashed dotted
border-left-color: currentColor transparent
border-left-style: none solid dashed dotted double groove ridge inset outset hidden
border-left-width: thin medium thick
border-radius:
border-right: none solid dashed dotted
border-right-color: currentColor transparent
border-right-style: none solid dashed dotted double groove ridge inset outset hidden
border-right-width: thin medium thick
border-spacing:
border-style: none solid dashed dotted double groove ridge inset outset hidden
border-top: none solid dashed dotted
border-top-color: currentColor transparent
border-top-left-radius:
border-top-right-radius:
border-top-style: none solid dashed dotted double groove ridge inset outset hidden
border-top-width: thin medium thick
border-width: thin medium thick
bottom: auto
box-shadow: none inset
box-sizing: content-box border-box
break-inside: auto avoid avoid-page avoid-column
caption-side: top bottom
caret-color: auto transparent
clear: none left right both inline-start inline-end
clip-path: none
color: currentColor transparent
column-count: auto
column-gap: normal
columns: auto
content: none normal open-quote close-quote no-open-quote no-close-quote
counter-increment: none
counter-reset: none
cursor: auto default none pointer context-menu help progress wait cell crosshair text vertical-text alias copy move no-drop not-allowed grab grabbing all-scroll col-resize row-resize n-resize e-resize s-resize w-resize ne-resize nw-resize se-resize sw-resize ew-resize ns-resize nesw-resize nwse-resize zoom-in zoom-out
direction: ltr rtl
display: none block inline inline-block flex inline-flex grid inline-grid flow-root contents table table-row table-cell list-item
empty-cells: show hide
fill: none currentColor
filter: none
flex: none auto
flex-basis: auto content
flex-direction: row row-reverse column column-reverse
flex-flow: row row-reverse column column-reverse nowrap wrap wrap-reverse
flex-grow:
flex-shrink:
flex-wrap: nowrap wrap wrap-reverse
float: none left right inline-start inline-end
font: caption icon menu message-box small-caption status-bar
font-family: serif sans-serif monospace cursive fantasy system-ui ui-sans-serif ui-serif ui-monospace
font-feature-settings: normal
font-size: xx-small x-small small medium large x-large xx-large smaller larger
font-stretch: normal condensed expanded semi-condensed semi-expanded
font-style: normal italic oblique
font-variant: normal small-caps
font-variant-numeric: normal tabular-nums lining-nums oldstyle-nums
font-weight: normal bold bolder lighter 100 200 300 400 500 600 700 800 900
gap: normal
grid: none
grid-area: auto
grid-auto-columns: auto min-content max-content
grid-auto-flow: row column dense
grid-auto-rows: auto min-content max-content
grid-column: auto span
grid-column-end: auto span
grid-column-start: auto span
grid-row: auto span
grid-row-end: auto span
grid-row-start: auto span
grid-template: none
grid-template-areas: none
grid-template-columns: none auto min-content max-content subgrid
grid-template-rows: none auto min-content max-content subgrid
height: auto fit-content min-content max-content
hyphens: none manual auto
image-rendering: auto crisp-edges pixelated smooth
inline-size: auto fit-content min-content max-content
inset: auto
inset-block: auto
inset-inline: auto
isolation: auto isolate
justify-content: normal start end center flex-start flex-end left right space-between space-around space-evenly stretch
justify-items: normal stretch start end center left right baseline legacy
justify-self: auto normal stretch start end center left right baseline
left: auto
letter-spacing: normal
line-clamp: none
line-height: normal
list-style: none disc circle square decimal inside outside
list-style-position: inside outside
list-style-type: none disc circle square decimal decimal-leading-zero lower-roman upper-roman lower-alpha upper-alpha
margin: auto
margin-block: auto
margin-bottom: auto
margin-inline: auto
margin-left: auto
margin-right: auto
margin-top: auto
mask: none
max-height: none fit-content min-content max-content
max-width: none fit-content min-content max-content
min-height: auto fit-content min-content max-content
min-width: auto fit-content min-content max-content
mix-blend-mode: normal multiply screen overlay darken lighten color-dodge color-burn hard-light soft-light difference exclusion hue saturation color luminosity
object-fit: fill contain cover none scale-down
object-position: top bottom left right center
opacity:
order:
outline: none solid dashed dotted auto
outline-color: currentColor invert
outline-offset:
outline-style: none auto solid dashed dotted double groove ridge inset outset
outline-width: thin medium thick
overflow: visible hidden clip scroll auto
overflow-wrap: normal break-word anywhere
overflow-x: visible hidden clip scroll auto
overflow-y: visible hidden clip scroll auto
overscroll-behavior: auto contain none
padding:
padding-block:
padding-bottom:
padding-inline:
padding-left:
padding-right:
padding-top:
perspective: none
place-content: normal start end center space-between space-around space-evenly stretch
place-items: normal stretch start end center baseline
place-self: auto normal stretch start end center
pointer-events: auto none visiblePainted visibleFill visibleStroke visible painted fill stroke all
position: static relative absolute fixed sticky
quotes: none auto
resize: none both horizontal vertical block inline
right: auto
rotate: none
row-gap: normal
scale: none
scroll-behavior: auto smooth
scroll-margin:
scroll-padding: auto
scroll-snap-align: none start end center
scroll-snap-type: none x y block inline both mandatory proximity
scrollbar-color: auto
scrollbar-gutter: auto stable
scrollbar-width: auto thin none
stroke: none currentColor
stroke-width:
tab-size:
table-layout: auto fixed
text-align: left right center justify start end match-parent
text-decoration: none underline overline line-through
text-decoration-color: currentColor
text-decoration-line: none underline overline line-through
text-decoration-style: solid double dotted dashed wavy
text-indent:
text-overflow: clip ellipsis
text-shadow: none
text-transform: none capitalize uppercase lowercase full-width
text-underline-offset: auto
top: auto
touch-action: auto none pan-x pan-y manipulation pinch-zoom
transform: none
transform-origin: top bottom left right center
transform-style: flat preserve-3d
transition: none all
transition-delay:
transition-duration:
transition-property: none all
transition-timing-function: ease ease-in ease-out ease-in-out linear step-start step-end
translate: none
user-select: auto none text all contain
vertical-align: baseline sub super text-top text-bottom middle top bottom
visibility: visible hidden collapse
white-space: normal nowrap pre pre-wrap pre-line break-spaces
width: auto fit-content min-content max-content
will-change: auto scroll-position contents transform opacity
word-break: normal break-all keep-all break-word
word-spacing: normal
writing-mode: horizontal-tb vertical-rl vertical-lr
z-index: auto
zoom: normal";

/// What every property takes.
pub const GLOBAL_VALUES: &[&str] = &["inherit", "initial", "unset", "revert", "revert-layer"];

pub const PSEUDO_CLASSES: &[&str] = &[
    "active",
    "any-link",
    "checked",
    "default",
    "defined",
    "disabled",
    "empty",
    "enabled",
    "first-child",
    "first-of-type",
    "focus",
    "focus-visible",
    "focus-within",
    "fullscreen",
    "has()",
    "hover",
    "in-range",
    "indeterminate",
    "invalid",
    "is()",
    "last-child",
    "last-of-type",
    "link",
    "not()",
    "nth-child()",
    "nth-last-child()",
    "nth-last-of-type()",
    "nth-of-type()",
    "only-child",
    "only-of-type",
    "optional",
    "out-of-range",
    "placeholder-shown",
    "read-only",
    "read-write",
    "required",
    "root",
    "target",
    "valid",
    "visited",
    "where()",
];

pub const PSEUDO_ELEMENTS: &[&str] = &[
    "after",
    "backdrop",
    "before",
    "file-selector-button",
    "first-letter",
    "first-line",
    "marker",
    "placeholder",
    "selection",
];

/// At-rules: (name, for which dialects: c CSS, s SCSS and Sass, l Less).
pub const AT_RULES: &[(&str, &str)] = &[
    ("charset", "csl"),
    ("container", "csl"),
    ("font-face", "csl"),
    ("import", "csl"),
    ("keyframes", "csl"),
    ("layer", "csl"),
    ("media", "csl"),
    ("namespace", "csl"),
    ("page", "csl"),
    ("property", "csl"),
    ("supports", "csl"),
    ("content", "s"),
    ("debug", "s"),
    ("each", "s"),
    ("else", "s"),
    ("error", "s"),
    ("extend", "s"),
    ("for", "s"),
    ("forward", "s"),
    ("function", "s"),
    ("if", "s"),
    ("include", "s"),
    ("mixin", "s"),
    ("return", "s"),
    ("use", "s"),
    ("warn", "s"),
    ("while", "s"),
    ("plugin", "l"),
];

/// Functions of values: (name, arguments shown).
pub const FUNCTIONS: &[(&str, &str)] = &[
    ("attr", "name"),
    ("calc", "expression"),
    ("clamp", "min, value, max"),
    ("color-mix", "in srgb, color percent, color"),
    ("conic-gradient", "angle, stops"),
    ("counter", "name"),
    ("cubic-bezier", "x1, y1, x2, y2"),
    ("env", "name"),
    ("fit-content", "size"),
    ("hsl", "h s l / alpha"),
    ("hwb", "h w b / alpha"),
    ("lab", "l a b / alpha"),
    ("lch", "l c h / alpha"),
    ("linear-gradient", "angle, stops"),
    ("max", "values"),
    ("min", "values"),
    ("minmax", "min, max"),
    ("oklch", "l c h / alpha"),
    ("radial-gradient", "shape, stops"),
    ("repeat", "count, tracks"),
    ("rgb", "r g b / alpha"),
    ("rgba", "r, g, b, alpha"),
    ("rotate", "angle"),
    ("scale", "x, y"),
    ("steps", "count, position"),
    ("translate", "x, y"),
    ("url", "address"),
    ("var", "--name, fallback"),
];

/// Color names.
pub const COLORS: &[&str] = &[
    "black",
    "white",
    "red",
    "green",
    "blue",
    "yellow",
    "orange",
    "purple",
    "pink",
    "gray",
    "grey",
    "silver",
    "maroon",
    "navy",
    "teal",
    "olive",
    "lime",
    "aqua",
    "fuchsia",
    "brown",
    "gold",
    "indigo",
    "violet",
    "coral",
    "crimson",
    "tomato",
    "salmon",
    "khaki",
    "beige",
    "ivory",
    "lavender",
    "turquoise",
    "tan",
    "chocolate",
    "darkgray",
    "lightgray",
    "whitesmoke",
    "rebeccapurple",
];

/// The properties, sorted, with the values each takes.
pub fn properties() -> impl Iterator<Item = (&'static str, Vec<&'static str>)> {
    PROPERTIES.lines().filter_map(|line| {
        let (name, values) = line.split_once(':')?;
        Some((name.trim(), values.split_whitespace().collect()))
    })
}

/// The values of a property, the colors when it takes one.
pub fn values_of(property: &str) -> Vec<&'static str> {
    let mut values = properties()
        .find(|(name, _)| *name == property)
        .map(|(_, v)| v)
        .unwrap_or_default();
    if property.ends_with("color")
        || matches!(
            property,
            "color" | "background" | "border" | "fill" | "stroke" | "outline"
        )
    {
        values.extend(COLORS);
    }
    values
}
