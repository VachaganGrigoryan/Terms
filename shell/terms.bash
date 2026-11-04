# Source explicitly, then call terms_enable [--track]. No startup files are changed.
_terms_decode() {
    local encoded=$1 escapes= pair
    TERMS_DECODED=
    case $encoded in *[!0123456789abcdef]*|'') return 2;; esac
    [ $((${#encoded} % 2)) -eq 0 ] || return 2
    while [ -n "$encoded" ]; do
        pair=${encoded:0:2}; encoded=${encoded:2}
        [ "$pair" != 00 ] || return 2
        escapes=$escapes\\x$pair
    done
    printf -v TERMS_DECODED '%b' "$escapes"
}
_terms_track() {
    local status=$? physical
    if [ "${_TERMS_TRACKING:-0}" = 1 ] && [ "${_TERMS_LAST_PWD-}" != "$PWD" ]; then
        if "$_TERMS_BIN" jump add "$PWD" >/dev/null; then _TERMS_LAST_PWD=$PWD; fi
    fi
    return "$status"
}
_terms_jump() {
    local interactive=$1 encoded status
    shift
    if [ "$interactive" = no ] && { [ $# -eq 0 ] || { [ $# -eq 1 ] && [ -d "$1" ]; }; }; then
        if [ $# -eq 0 ]; then builtin cd -- "$HOME" || return; else builtin cd -- "$1" || return; fi
    else
        if [ "$interactive" = yes ]; then
            encoded=$("$_TERMS_BIN" jump query --interactive --hex -- "$@")
        else
            encoded=$("$_TERMS_BIN" jump query --hex -- "$@")
        fi
        status=$?; [ "$status" -eq 0 ] || return "$status"
        _terms_decode "$encoded" || return 2
        builtin cd -- "$TERMS_DECODED" || return
    fi
    _terms_track
}
_terms_complete() {
    local encoded line
    COMPREPLY=()
    encoded=$("$_TERMS_BIN" jump list --hex) || return
    while IFS= read -r line; do
        _terms_decode "$line" || continue
        case $TERMS_DECODED in "${COMP_WORDS[COMP_CWORD]}"*) COMPREPLY[${#COMPREPLY[@]}]=$TERMS_DECODED;; esac
    done <<< "$encoded"
}
_terms_file_hex() {
    local record i total=0 LC_ALL=C
    "$_TERMS_BIN" search --files --null | while IFS= read -r -d '' record; do
        total=$((total + ${#record}))
        [ "$total" -le 1048576 ] || return 2
        for ((i=0; i<${#record}; i++)); do printf '%02x' "'${record:i:1}"; done
        printf '\n'
    done
}
_terms_insert() {
    local encoded candidates line quoted result= prior=$READLINE_LINE cursor=$READLINE_POINT
    # Complete and validate enumeration before opening the interactive selector.
    candidates=$(set -o pipefail; _terms_file_hex) || return
    encoded=$(set +o pipefail; while IFS= read -r line; do
        [ -n "$line" ] || continue
        _terms_decode "$line" || exit 2
        printf '%s\0' "$TERMS_DECODED"
    done <<< "$candidates" | "$_TERMS_BIN" pick --read0 --hex) || return
    while IFS= read -r line; do
        _terms_decode "$line" || return 2
        printf -v quoted '%q' "$TERMS_DECODED"
        result=$result$quoted' '
    done <<< "$encoded"
    READLINE_LINE=${prior:0:cursor}$result${prior:cursor}
    READLINE_POINT=$((cursor + ${#result}))
}
terms_enable() {
    local requested=${1-}
    [ "$requested" = '' ] || [ "$requested" = --track ] || return 2
    if [ "${_TERMS_ENABLED:-0}" != 1 ]; then
        _TERMS_BIN=${TERMS_BIN:-$(type -P terms)}
        if [ "${_TERMS_BIN#/}" = "$_TERMS_BIN" ] || [ ! -x "$_TERMS_BIN" ]; then
            if [ -n "${TERMS_BIN-}" ]; then
                printf 'terms: TERMS_BIN=%q is not an absolute path to an executable; fix it, or run: unset TERMS_BIN\n' "$TERMS_BIN" >&2
            else
                printf '%s\n' 'terms: cannot find terms on PATH; set TERMS_BIN to its absolute path' >&2
            fi
            return 2
        fi
        declare -F tj >/dev/null && { printf '%s\n' 'terms: tj already exists' >&2; return 2; }
        declare -F tji >/dev/null && { printf '%s\n' 'terms: tji already exists' >&2; return 2; }
        tj() { _terms_jump no "$@"; }
        tji() { _terms_jump yes "$@"; }
        complete -o filenames -F _terms_complete tj tji
        _TERMS_TJ_DEF=$(declare -f tj)
        _TERMS_TJI_DEF=$(declare -f tji)
        # Short names for the other commands. An occupied name is skipped, never replaced.
        # The eval input is this fixed table only; Bash 3.2 has no associative arrays.
        local name sub spec
        _TERMS_SHORT=
        for spec in t: tl:ls ts:search tq:json tp:pick th:history tg:git; do
            name=${spec%%:*}; sub=${spec#*:}
            if [ -n "$(type -t "$name")" ]; then
                printf '%s\n' "terms: $name already exists; not defined" >&2
                continue
            fi
            eval "$name() { \"\$_TERMS_BIN\" $sub \"\$@\"; }"
            printf -v "_TERMS_DEF_$name" '%s' "$(declare -f "$name")"
            _TERMS_SHORT="$_TERMS_SHORT $name"
        done
        _TERMS_ENABLED=1
    fi
    if [ "$requested" = --track ] && [ "${_TERMS_TRACKING:-0}" != 1 ]; then
        # Reject array PROMPT_COMMAND rather than flattening existing hooks.
        case $(declare -p PROMPT_COMMAND 2>/dev/null) in 'declare -a'*) printf '%s\n' 'terms: array PROMPT_COMMAND tracking is unsupported' >&2; return 2;; esac
        _TERMS_PREVIOUS_PROMPT=${PROMPT_COMMAND-}
        _TERMS_OWN_PROMPT='_terms_track;'$_TERMS_PREVIOUS_PROMPT
        PROMPT_COMMAND=$_TERMS_OWN_PROMPT
        _TERMS_TRACKING=1
        _terms_track
    fi
}
_terms_bound_jump() { [ "${_TERMS_ENABLED:-0}" = 1 ] && tji; }
_terms_binding_has() {
    local bindings
    bindings=$(bind -p; bind -s; bind -X 2>/dev/null)
    case $bindings in *"$1"*) return 0;; *) return 1;; esac
}
terms_bindings_enable() {
    [ "${_TERMS_ENABLED:-0}" = 1 ] || return 2
    [ "${_TERMS_BINDINGS:-0}" = 1 ] && return 0
    [ "${1-}" = '' ] || [ "${1-}" = --replace ] || return 2
    if ! bind -X >/dev/null 2>&1 && [ "${1-}" != --replace ]; then
        printf '%s\n' 'terms: Bash 3.2 cannot inspect shell bindings; explicit --replace required' >&2
        return 2
    fi
    if _terms_binding_has '\ec' && [ "${1-}" != --replace ]; then
        printf '%s\n' 'terms: Alt-C is occupied' >&2; return 2
    fi
    bind -x '"\ec":_terms_bound_jump' || return
    _TERMS_BINDINGS=1
    if [ "${BASH_VERSINFO[0]}" -ge 4 ]; then
        if ! _terms_binding_has '\C-t'; then bind -x '"\C-t":_terms_insert'; fi
    fi
}
terms_disable() {
    if [ "${PROMPT_COMMAND-}" = "${_TERMS_OWN_PROMPT-unset}" ]; then PROMPT_COMMAND=$_TERMS_PREVIOUS_PROMPT; fi
    # Remove only bindings still associated with Terms.
    if [ "${BASH_VERSINFO[0]}" -ge 4 ] && _terms_binding_has '"\ec": "_terms_bound_jump"'; then bind -r '\ec'; fi
    if [ "${BASH_VERSINFO[0]}" -ge 4 ] && _terms_binding_has '"\C-t": "_terms_insert"'; then bind -r '\C-t'; fi
    local name definition
    for name in tj tji; do
        definition=$(complete -p "$name" 2>/dev/null)
        case $definition in *'-F _terms_complete '*) complete -r "$name";; esac
    done
    [ "$(declare -f tj)" != "${_TERMS_TJ_DEF-}" ] || unset -f tj
    [ "$(declare -f tji)" != "${_TERMS_TJI_DEF-}" ] || unset -f tji
    local def
    for name in ${_TERMS_SHORT-}; do
        def=_TERMS_DEF_$name
        [ "$(declare -f "$name")" != "${!def}" ] || unset -f "$name"
        unset "$def"
    done
    unset _TERMS_SHORT
    unset _TERMS_ENABLED _TERMS_TRACKING _TERMS_LAST_PWD _TERMS_BINDINGS _TERMS_INSERT_BINDING
}
