; Scopes
(fn_decl) @local.scope
(block)   @local.scope

; Definitions
(fn_decl    name: (identifier) @local.definition)
(param      name: (identifier) @local.definition)
(var_stmt   name: (identifier) @local.definition)
(const_stmt name: (identifier) @local.definition)

; References
(identifier) @local.reference
