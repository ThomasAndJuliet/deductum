#import "../../packages/typst/chartgen.typ": chart
#let engine = plugin("../../target/wasm32-unknown-unknown/debug/chartgen_typst.wasm")
#set page(width: 550pt, height: auto, margin: 20pt)
#set text(font: "Libertinus Serif", size: 10pt)
#chart(json("../basic.json"), engine)
