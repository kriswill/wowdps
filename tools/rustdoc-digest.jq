# rustdoc JSON (format_version 61) -> Markdown digest, one section per
# source file. Emits "@@@FILE@@@ <path>" marker lines the caller splits on.
#
# Renders every item of the local crate that has a public path: structs,
# enums, traits, free functions, aliases, constants, statics, macros and
# doc-bearing modules — signatures plus their doc comments — with each type's
# inherent methods under it and its trait impls listed by name. Auto-trait and
# blanket impls are dropped. Runs of near-identical methods (Styled's
# generated `h_0` … `h_neg_80`) collapse to one line listing the names.

.index as $ix
| .paths as $paths
| .crate_version as $ver

# ---- types ------------------------------------------------------------
# One recursive dispatcher: jq has no forward references, and generic
# args, bounds and types all recurse into each other.
| def r($m):
    if $m == "gargs" then
      if . == null then ""
      elif .angle_bracketed then
        ([.angle_bracketed.args[]? | r("garg")] + [.angle_bracketed.constraints[]? | r("constraint")])
        | if length == 0 then "" else "<" + join(", ") + ">" end
      elif .parenthesized then
        "(" + ([.parenthesized.inputs[]? | r("ty")] | join(", ")) + ")"
        + (if .parenthesized.output then " -> " + (.parenthesized.output | r("ty")) else "" end)
      else "" end
    elif $m == "garg" then
      if type == "string" then "_"
      elif .type then .type | r("ty")
      elif .lifetime then .lifetime
      elif .const then (.const.expr // "_")
      else "_" end
    elif $m == "constraint" then
      .name + (.args | r("gargs"))
      + (if .binding.equality then
           " = " + (.binding.equality | if .type then (.type | r("ty")) else (.constant.expr // "_") end)
         elif .binding.constraint then ": " + (.binding.constraint | map(r("bound")) | join(" + "))
         else "" end)
    elif $m == "bound" then
      if .trait_bound then .trait_bound
        | (if ((.generic_params // []) | length) > 0
             then "for<" + (.generic_params | map(.name) | join(", ")) + "> " else "" end)
          + (if .modifier == "maybe" then "?" elif .modifier == "maybe_const" then "~const " else "" end)
          + .trait.path + (.trait.args | r("gargs"))
      elif .outlives then .outlives
      elif .use then "use<" + (.use | map(if type == "string" then . else (.lifetime // .param // tostring) end) | join(", ")) + ">"
      else tojson end
    else # "ty"
      if type == "string" then (if . == "infer" then "_" else . end)
      elif .resolved_path then .resolved_path | .path + (.args | r("gargs"))
      elif .generic then .generic
      elif .primitive then .primitive
      elif .borrowed_ref then .borrowed_ref
        | "&" + (if .lifetime then .lifetime + " " else "" end)
          + (if .is_mutable then "mut " else "" end) + (.type | r("ty"))
      elif .tuple then
        "(" + (.tuple | map(r("ty")) | join(", ")) + (if (.tuple | length) == 1 then "," else "" end) + ")"
      elif .slice then "[" + (.slice | r("ty")) + "]"
      elif .array then "[" + (.array.type | r("ty")) + "; " + .array.len + "]"
      elif .impl_trait then "impl " + (.impl_trait | map(r("bound")) | join(" + "))
      elif .dyn_trait then
        "dyn " + ([.dyn_trait.traits[] | .trait.path + (.trait.args | r("gargs"))]
                  + (if .dyn_trait.lifetime then [.dyn_trait.lifetime] else [] end) | join(" + "))
      elif .raw_pointer then
        "*" + (if .raw_pointer.is_mutable then "mut " else "const " end) + (.raw_pointer.type | r("ty"))
      elif .qualified_path then .qualified_path
        | "<" + (.self_type | r("ty"))
          + (if .trait then " as " + .trait.path + (.trait.args | r("gargs")) else "" end)
          + ">::" + .name + (.args | r("gargs"))
      elif .function_pointer then .function_pointer
        | (if .header.is_unsafe then "unsafe " else "" end)
          + "fn(" + (.sig.inputs | map(.[1] | r("ty")) | join(", ")) + ")"
          + (if .sig.output then " -> " + (.sig.output | r("ty")) else "" end)
      elif .pat then .pat.type | r("ty")
      else tojson end
    end;
  def ty: r("ty");
  def gargs: r("gargs");
  def bound: r("bound");

# ---- generics ---------------------------------------------------------
  def gparam:
    if .kind.lifetime then
      .name + (if ((.kind.lifetime.outlives // []) | length) > 0 then ": " + (.kind.lifetime.outlives | join(" + ")) else "" end)
    elif .kind.type then
      .name + (if (.kind.type.bounds | length) > 0 then ": " + (.kind.type.bounds | map(bound) | join(" + ")) else "" end)
      + (if .kind.type.default then " = " + (.kind.type.default | ty) else "" end)
    elif .kind.const then "const " + .name + ": " + (.kind.const.type | ty)
    else .name end;
  def gparams:
    [(.params // [])[] | select((.kind.type.is_synthetic // false) | not) | gparam]
    | if length == 0 then "" else "<" + join(", ") + ">" end;
  def wpred:
    if .bound_predicate then .bound_predicate | (.type | ty) + ": " + (.bounds | map(bound) | join(" + "))
    elif .lifetime_predicate then .lifetime_predicate | .lifetime + ": " + (.outlives | join(" + "))
    elif .eq_predicate then .eq_predicate
      | (.lhs | ty) + " = " + (.rhs | if .type then (.type | ty) else (.constant.expr // "_") end)
    else tojson end;
  def wherec:
    [(.where_predicates // [])[] | wpred] | if length == 0 then "" else " where " + join(", ") end;

# ---- signatures -------------------------------------------------------
  def selfarg:
    if .generic == "Self" then "self"
    elif .borrowed_ref and .borrowed_ref.type.generic == "Self" then
      "&" + (if .borrowed_ref.lifetime then .borrowed_ref.lifetime + " " else "" end)
      + (if .borrowed_ref.is_mutable then "mut " else "" end) + "self"
    else "self: " + ty end;
  def fnsig($name):
    .inner.function as $f
    | ($f.header | (if .is_const then "const " else "" end)
                 + (if .is_async then "async " else "" end)
                 + (if .is_unsafe then "unsafe " else "" end))
      + "fn " + $name + ($f.generics | gparams) + "("
      + ($f.sig.inputs | map(if .[0] == "self" then (.[1] | selfarg) else .[0] + ": " + (.[1] | ty) end) | join(", "))
      + ")" + (if $f.sig.output then " -> " + ($f.sig.output | ty) else "" end)
      + ($f.generics | wherec);
  def firstline: (.docs // "") | split("\n") | map(select(test("\\S"))) | (.[0] // "");
  def docblock: if (.docs // "") | test("\\S") then "\n" + .docs + "\n" else "" end;

# A member (method, assoc type/const) inside an impl or trait.
  def member:
    if .inner.function then "`" + fnsig(.name) + (if .inner.function.has_body then "" else ";" end) + "`"
    elif .inner.assoc_type then
      "`type " + .name + (if (.inner.assoc_type.bounds | length) > 0 then ": " + (.inner.assoc_type.bounds | map(bound) | join(" + ")) else "" end)
      + (if .inner.assoc_type.type then " = " + (.inner.assoc_type.type | ty) else "" end) + "`"
    elif .inner.assoc_const then
      "`const " + .name + ": " + (.inner.assoc_const.type | ty)
      + (if .inner.assoc_const.value then " = " + .inner.assoc_const.value else "" end) + "`"
    else "`" + (.name // "?") + "`" end;
  # Same signature with the name blanked — the key a generated family shares.
  def shape: if .inner.function then fnsig("_") else (.inner | keys[0]) end;
  def members:
    [ .[] | select(. != null) ]
    | to_entries
    | group_by(.value | firstline + "\u0001" + shape)
    | map(if length >= 4 and (.[0].value | firstline) != "" then
            {at: .[0].key,
             md: "- " + (map("`" + .value.name + "`") | join(", "))
                 + " (" + (length | tostring) + " generated variants, each `"
                 + (.[0].value | shape | sub("fn _"; "fn ")) + "`)\n\n  "
                 + (.[0].value | firstline) + "\n"}
          else .[] | {at: .key, md: "- " + (.value | member) + "\n" + (.value | docblock | gsub("\n"; "\n  "))}
          end)
    | sort_by(.at) | map(.md) | join("\n");

# ---- impls ------------------------------------------------------------
  def impls_of($ids):
    [ $ids[]? | $ix[tostring] | select(. != null) | .inner.impl
      | select(.is_synthetic | not) | select(.blanket_impl == null) | select(.is_negative | not) ] as $imps
    | ([ $imps[] | select(.trait == null) | .items[] | $ix[tostring]
         | select(. != null) | select(.visibility == "public") ] ) as $inherent
    | ([ $imps[] | select(.trait != null) | .trait.path + (.trait.args | gargs) ] | unique) as $traits
    | (if ($inherent | length) > 0 then "\n**Methods**\n\n" + ($inherent | members) + "\n" else "" end)
      + (if ($traits | length) > 0 then "\n**Trait impls:** " + ($traits | map("`" + . + "`") | join(", ")) + "\n" else "" end);

# ---- items ------------------------------------------------------------
  def field:
    "- `" + .name + ": " + (.inner.struct_field | ty) + "`"
    + (if (.docs // "") | test("\\S") then " — " + (.docs | gsub("\n"; "\n  ")) else "" end);
  def variant:
    .inner.variant.kind as $k
    | "- `" + .name
      + (if $k == "plain" then ""
         elif $k.tuple then "(" + ($k.tuple | map(if . == null then "_" else ($ix[tostring].inner.struct_field | ty) end) | join(", ")) + ")"
         elif $k.struct then " { " + ($k.struct.fields | map($ix[tostring] | .name + ": " + (.inner.struct_field | ty)) | join(", ")) + " }"
         else "" end)
      + (if .inner.variant.discriminant then " = " + .inner.variant.discriminant.expr else "" end) + "`"
      + (if (.docs // "") | test("\\S") then " — " + (.docs | gsub("\n"; "\n  ")) else "" end);
  def render($path):
    (.inner | keys[0]) as $kind
    | .inner[$kind] as $i
    | (if $kind == "struct" then
         "### struct `" + .name + ($i.generics | gparams) + "`"
         + (if ($i.kind | type) == "object" and $i.kind.tuple then " (" + ($i.kind.tuple | map(if . == null then "_" else ($ix[tostring].inner.struct_field | ty) end) | join(", ")) + ")"
            elif $i.kind == "unit" then " (unit)" else "" end)
       elif $kind == "enum" then "### enum `" + .name + ($i.generics | gparams) + "`"
       elif $kind == "union" then "### union `" + .name + ($i.generics | gparams) + "`"
       elif $kind == "trait" then
         "### trait `" + .name + ($i.generics | gparams)
         + (if ($i.bounds | length) > 0 then ": " + ($i.bounds | map(bound) | join(" + ")) else "" end) + "`"
       elif $kind == "function" then "### fn `" + fnsig(.name) + "`"
       elif $kind == "type_alias" then "### type `" + .name + ($i.generics | gparams) + " = " + ($i.type | ty) + "`"
       elif $kind == "constant" then "### const `" + .name + ": " + ($i.type | ty) + " = " + ($i.const.expr // "…") + "`"
       elif $kind == "static" then "### static `" + .name + ": " + ($i.type | ty) + "`"
       elif $kind == "macro" then "### macro `" + .name + "!`"
       elif $kind == "proc_macro" then "### proc macro `" + .name + "` (" + ($i.kind // "") + ")"
       elif $kind == "module" then "### mod `" + .name + "`"
       else "### " + $kind + " `" + (.name // "?") + "`" end)
      + "\n\n`" + $path + "`"
      + (if .deprecation then "  **deprecated**" + (if .deprecation.note then ": " + .deprecation.note else "" end) else "" end)
      + "\n" + docblock
      + (if $kind == "struct" and ($i.kind | type) == "object" and $i.kind.plain then
           [ $i.kind.plain.fields[] | $ix[tostring] | select(.visibility == "public") | field ]
           | if length > 0 then "\n**Fields**\n\n" + join("\n") + "\n" else "" end
         else "" end)
      + (if $kind == "enum" then "\n**Variants**\n\n" + ([ $i.variants[] | $ix[tostring] | variant ] | join("\n")) + "\n" else "" end)
      + (if $kind == "trait" then
           ([ $i.items[] | $ix[tostring] | select(. != null) ] as $ms
            | if ($ms | length) > 0 then "\n**Items**\n\n" + ($ms | members) + "\n" else "" end)
           + (([ $i.implementations[]? | $ix[tostring] | select(. != null) | .inner.impl
                 | select(.is_synthetic | not) | select(.blanket_impl == null) | .for | ty ] | unique) as $for
              | if ($for | length) > 0 then
                  "\n**Implemented for:** " + ($for[:60] | map("`" + . + "`") | join(", "))
                  + (if ($for | length) > 60 then ", … (" + ($for | length | tostring) + " total)" else "" end) + "\n"
                else "" end)
         else "" end)
      + (if $kind == "struct" or $kind == "enum" or $kind == "union" then impls_of($i.impls) else "" end);

# ---- walk -------------------------------------------------------------
  [ $paths | to_entries[]
    | select(.value.crate_id == 0)
    | .value.path as $p | .value.kind as $k
    | $ix[.key] | select(. != null)
    | select($k != "module" or ((.docs // "") | test("\\S")))
    | select(.inner | keys[0] | IN("struct","enum","union","trait","function","type_alias","constant","static","macro","proc_macro","module"))
    | {file: (.span.filename // "(no span)"), line: (.span.begin[0] // 0), path: ($p | join("::")), kind: $k, item: .} ]
  | group_by(.file)
  | .[]
  | sort_by(.line)
  | "@@@FILE@@@ " + .[0].file,
    "# `" + .[0].file + "` (" + ($ver // "") + ")\n",
    ( .[] | . as $r | $r.item | render($r.path) ),
    "@@@INDEX@@@ " + (map(.kind + "\t" + .path) | join("\n@@@INDEX@@@ "))
