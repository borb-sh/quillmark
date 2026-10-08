#import "@local/quillmark-helper:0.1.0": data

// `table_demo` with every knob declared: the parity corpus renders each entry
// through both, so a knob's declared lowering compiles where the undeclared
// one lays the table out as if it were absent.
#underline(data.title)

#data.at("$body")
