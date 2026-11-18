#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
  printf 'Usage: check-rust-formatting.bash FILE.rs [FILE.rs ...]\n' >&2
  exit 2
fi

result=0
for file in "$@"; do
  [[ "/$file" != */generated/* ]] || continue
  if [[ "$file" != *.rs || ! -f "$file" || ! -r "$file" ]]; then
    printf 'Expected a readable Rust file: %s\n' "$file" >&2
    exit 2
  fi

  status=0
  LC_ALL=C RUST_SOURCE="$file" awk '
    function trim_left(text) {
      sub(/^[[:space:]]+/, "", text)
      return text
    }

    function code_text(text,    code_text_so_far, token, end) {
      while (text != "") {
        if (comment_depth) {
          if (!match(text, /\/\*|\*\//)) break
          comment_depth += substr(text, RSTART, 1) == "/" ? 1 : -1
          text = substr(text, RSTART + RLENGTH)
        } else if (raw_end != "") {
          end = index(text, raw_end)
          if (!end) break
          text = substr(text, end + length(raw_end))
          raw_end = ""
        } else if (in_string) {
          if (!match(text, /^([^"\\]|\\.)*"/)) break
          text = substr(text, RLENGTH + 1)
          in_string = 0
        } else {
          if (!match(text, /\/\/|\/\*|[bc]?r#*"|[bc]?"|b?\047([^\047\\]|\\(x[[:xdigit:]][[:xdigit:]]|u\{[[:xdigit:]_]+\}|.))\047/)) {
            return code_text_so_far text
          }
          code_text_so_far = code_text_so_far substr(text, 1, RSTART - 1)
          token = substr(text, RSTART, RLENGTH)
          text = substr(text, RSTART + RLENGTH)
          if (token == "//") break
          if (token == "/*") {
            code_text_so_far = code_text_so_far " "
            comment_depth = 1
          } else {
            code_text_so_far = code_text_so_far " \" "
            if (token ~ /^[bc]?"$/) {
              in_string = 1
            } else if (token ~ /^[bc]?r/) {
              sub(/^[bc]/, "", token)
              raw_end = "\"" substr(token, 2, length(token) - 2)
            }
          }
        }
      }
      return code_text_so_far
    }

    function shares_identifier(text, previous,    count, parts, p, present, ident, raw) {
      count = split(previous, parts, /[^a-zA-Z0-9_#]+/)
      for (p = 1; p <= count; p++) {
        raw = sub(/^r#/, "", parts[p])
        if (raw || !(parts[p] in reserved)) present[parts[p]] = 1
      }

      while (match(text, /(r#)?[a-zA-Z_][a-zA-Z0-9_]*/)) {
        ident = substr(text, RSTART, RLENGTH)
        text = substr(text, RSTART + RLENGTH)
        raw = sub(/^r#/, "", ident)
        if ((raw || !(ident in reserved)) && ident in present) return 1
      }
      return 0
    }

    function first_body_line(start, keyword,    j, depth, pattern, operand, token, text, body) {
      pattern = keyword == "for"
      operand = keyword != "loop"
      for (j = start; j <= line_count; j++) {
        text = code[j]
        if (j == start) {
          text = trim_left(text)
          sub("^" label, "", text)
          sub("^" keyword "[[:space:]]+", "", text)
        }
        while (match(text, /[][(){}]|=>|&&|\|\||[;=!"]|(r#)?[a-zA-Z_0-9]+/)) {
          token = substr(text, RSTART, RLENGTH)
          text = substr(text, RSTART + RLENGTH)
          if (token == "{" && !depth && !pattern && !operand) {
            body = trim_left(text)
            return body == "" ? code[j + 1] : body
          }
          if (token == "(" || token == "[" || token == "{") {
            depth++
          } else if (token == ")" || token == "]" || token == "}") {
            if (depth == 0) return ""
            depth--
            operand = 0
          } else if (!depth) {
            if (token == "let") {
              pattern = 1
            } else if (token == "=" || token == "in") {
              pattern = 0
              operand = 1
            } else if (token == "=>") {
              return token
            } else if (token == ";") {
              return ""
            } else {
              operand = token ~ /^(unsafe|const|async|move|try|!|&&|\|\|)$/
            }
          }
        }
      }
      return ""
    }

    function in_impl_header(line,    j, text) {
      for (j = line - 1; j >= 1; j--) {
        text = code[j]
        if (text ~ /[;{}][[:space:]]*$/) return 0
        if (text ~ /^[[:space:]]*(unsafe[[:space:]]+)?impl([[:space:]<]|$)/) return 1
      }
      return 0
    }

    function flow_exempt(line, keyword, current,    previous, body) {
      previous = trim_left(code[line - 1])
      if (previous ~ /^[[:space:]]*$/ || previous ~ /\{[[:space:]]*$/) return 1
      if (previous_attribute) return 1
      if (keyword == "if" || keyword == "match" || keyword == "spawn") {
        if (previous ~ /=[[:space:]]*$/ && previous !~ /[=!<>]=[[:space:]]*$/) return 1
        if (previous ~ /[,(|][[:space:]]*$/) return 1
      }
      if (keyword == "if") {
        if (previous ~ /(^|\}[[:space:]]*)else[[:space:]]*$/) return 1
        if (previous ~ /\)[[:space:]]*$/) return 1
        if (previous !~ /;[[:space:]]*$/ &&
            (previous ~ /^\|/ || (previous !~ /\|\|/ &&
             previous ~ /[[:alnum:]][[:space:]]*\|[[:space:]]*[[:alnum:]]/))) return 1
      } else if (keyword == "spawn") {
        if (current ~ /^\./) return 1
        return shares_identifier(current, previous) ||
               (current !~ /[;,][[:space:]]*$/ && shares_identifier(code[line + 1], previous))
      } else if (keyword != "match" && previous ~ ("^" label)) {
        return 1
      }

      if (keyword == "for" && in_impl_header(line)) return 1
      sub("^" label, "", current)
      sub("^" keyword "[[:space:]]+", "", current)
      if (shares_identifier(current, previous)) return 1
      body = first_body_line(line, keyword)
      return (keyword == "if" && body == "=>") || shares_identifier(body, previous)
    }

    function check_flow(i,    current, keyword, k) {
      if (i < 2) return
      current = trim_left(code[i])
      for (k = 1; k <= keyword_count; k++) {
        keyword = keywords[k]
        if ((keyword == "spawn" ? trim_left(outside_macro) : current) ~ patterns[keyword] &&
            !flow_exempt(i, keyword, current)) {
          report(i, "Missing blank line above `" keyword "`")
        }
      }
    }

    function in_macro_body(text,    inside, token) {
      inside = macro_depth > 0
      outside_macro = ""
      while (text != "") {
        if (!macro_depth) {
          if (!match(text, /([a-zA-Z_][a-zA-Z0-9_]*![[:space:]]*|macro_rules![[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*[[:space:]]*)(\{|\(|\[)/)) {
            outside_macro = outside_macro text
            break
          }
          outside_macro = outside_macro substr(text, 1, RSTART - 1) " "
          macro_depth = 1
        } else {
          if (!match(text, /[][(){}]/)) break
          token = substr(text, RSTART, 1)
          macro_depth += token == "{" || token == "(" || token == "[" ? 1 : -1
        }
        text = substr(text, RSTART + RLENGTH)
      }
      return inside
    }

    function is_attribute_line(text,    token) {
      if (!attribute_depth) {
        if (text !~ /^[[:space:]]*#\[/) return 0
        attribute_top_level = text ~ /^#\[/
      }
      while (match(text, /[][]/)) {
        token = substr(text, RSTART, 1)
        attribute_depth += token == "[" ? 1 : -1
        if (!attribute_depth) break
        text = substr(text, RSTART + RLENGTH)
      }
      return 1
    }

    function reset_module_section() {
      previous_category = -1
      highest_category = -1
      previous_name = ""
      blank_lines = 0
      blank_run = 0
      blank_run_max = 0
      has_intervening_comment = 0
    }

    function has_direct_test(cfg,    start, rest, position, character, depth, token) {
      start = index(cfg, "#[cfg(all(")
      while (start > 0) {
        rest = substr(cfg, start + length("#[cfg(all("))
        depth = 0
        token = ""
        for (position = 1; position <= length(rest); position++) {
          character = substr(rest, position, 1)
          if (depth > 0) {
            if (character == "(") depth++
            if (character == ")") depth--
          } else if (character == "(") {
            depth = 1
            token = ""
          } else if (character == "," || character == ")") {
            if (token == "test") return 1
            token = ""
            if (character == ")") break
          } else {
            token = token character
          }
        }
        cfg = substr(rest, position + 1)
        start = index(cfg, "#[cfg(all(")
      }
      return 0
    }

    function module_category(text, cfg,    category) {
      if (cfg ~ /#\[macro_use\]/) return 0
      if (cfg ~ /#\[cfg\(test\)\]/ || has_direct_test(cfg)) return 7

      category = 3
      if (text ~ /^pub[[:space:]]/) {
        category = 1
      } else if (text ~ /^pub\(/) {
        category = 2
      }
      return cfg ~ /#\[cfg\(/ ? category + 3 : category
    }

    function check_module(i, attribute,    text, name, category, sort_name) {
      text = lines[i]
      if (attribute) {
        if (attribute_top_level) {
          blank_run = 0
          attributes = attributes code[i]
        }
        return
      }
      if (comments[i] && code[i] ~ /^[[:space:]]*$/) {
        if (previous_category >= 0) has_intervening_comment = 1
        blank_run = 0
        return
      }
      if (text ~ /^[[:space:]]*$/) {
        if (previous_category >= 0) {
          blank_lines++
          blank_run++
          if (blank_run > blank_run_max) blank_run_max = blank_run
        }
        return
      }
      if (code[i] !~ /^(pub(\([^)]*\))?[[:space:]]+)?mod[[:space:]]+(r#)?[A-Za-z_][A-Za-z0-9_]*[[:space:]]*;[[:space:]]*$/) {
        reset_module_section()
        attributes = ""
        return
      }

      name = code[i]
      sub(/[[:space:]]*;.*/, "", name)
      sub(/^.*mod[[:space:]]+/, "", name)
      sort_name = name
      sub(/^r#/, "", sort_name)
      gsub(/[[:space:]]/, "", attributes)
      category = module_category(text, attributes)
      if (previous_category >= 0 && category != previous_category && blank_lines != 1 &&
          !(has_intervening_comment && blank_lines > 0 && blank_run_max == 1)) {
        report(i, "Expected one blank line before " sections[category + 1] " module `" name "`")
      }
      if (category < highest_category) {
        report(i, "Module `" name "` is in the wrong section; expected order: " section_order)
      } else if (category > highest_category) {
        highest_category = category
      }
      if (category == previous_category && sort_name < previous_name) {
        report(i, "Module `" name "` is not alphabetized in the " sections[category + 1] " section")
      }

      previous_category = category
      previous_name = sort_name
      blank_lines = blank_run = blank_run_max = has_intervening_comment = 0
      attributes = ""
    }

    function report(line, message) {
      printf "%s:%d: %s\n", source, line, message > "/dev/stderr"
      violations++
    }

    BEGIN {
      quote = sprintf("%c", 39)
      label = quote "[a-zA-Z_][a-zA-Z0-9_\200-\377]*:[[:space:]]*"
      keyword_count = split("if match for while loop spawn", keywords, " ")
      for (k = 1; k < keyword_count; k++) {
        keyword = keywords[k]
        patterns[keyword] = "^(" label ")?" keyword "[[:space:]]"
      }
      patterns["spawn"] = "(^|\\.|::)spawn(_local|_blocking)?(::<.*>)?\\("

      words = "if else let mut ref true false return break continue match as in for while loop"
      words = words " fn struct enum impl trait pub use mod const static type where async await"
      words = words " move unsafe extern crate super dyn self Self spawn spawn_local spawn_blocking tokio _"
      word_count = split(words, names, " ")
      for (w = 1; w <= word_count; w++) reserved[names[w]] = 1

      section_order = "macro prelude, public, restricted, private, cfg-gated public"
      section_order = section_order ", cfg-gated restricted, cfg-gated private, test-only"
      split(section_order, sections, ", ")
      reset_module_section()

      source = ENVIRON["RUST_SOURCE"]
      while ((read_status = (getline text < source)) > 0) {
        lines[++line_count] = text
        comments[line_count] = (raw_end == "" && !in_string &&
                                (comment_depth || text ~ /^[[:space:]]*(\/\/|\/\*)/))
        code[line_count] = code_text(text)
      }
      if (read_status < 0) {
        print "Cannot read Rust file: " source > "/dev/stderr"
        exit 2
      }
      close(source)

      for (i = 1; i <= line_count; i++) {
        inside_macro = in_macro_body(code[i])
        attribute = (!inside_macro || attribute_depth) && is_attribute_line(code[i])
        if (!inside_macro && !attribute) check_flow(i)
        if (!inside_macro || attribute) check_module(i, attribute)
        previous_attribute = attribute
      }
      exit (violations ? 1 : 0)
    }
  ' || status=$?

  case "$status" in
    0) ;;
    1) result=1 ;;
    *) exit 2 ;;
  esac
done

exit "$result"
