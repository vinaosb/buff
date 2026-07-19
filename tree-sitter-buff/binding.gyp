{
  "targets": [
    {
      "target_name": "tree_sitter_buff_binding",
      "include_dirs": [
        "<!(node -e \"require('nan')\")",
        "src"
      ],
      "sources": [
        "bindings/node/binding.cc",
        "src/parser.c",
        "src/scanner.c"
      ],
      "cflags_c": [
        "-std=c99"
      ],
      "conditions": [
        [
          "OS!='win'",
          {
            "cflags_c": [
              "-std=c99"
            ]
          }
        ]
      ]
    }
  ]
}
