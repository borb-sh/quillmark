#import "@local/quillmark-helper:0.1.0": data

// `table_demo` with every knob and two elements declared: the parity corpus
// renders each entry through both, so a declared lowering compiles where the
// undeclared one lays the content out as if the knob or element were absent.
#underline(data.title)

#data.at("$body")
