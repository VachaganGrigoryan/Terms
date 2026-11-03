# Source explicitly, then call terms_enable [--track].
_terms_decode() {
    local encoded=$1 escapes='' pair
    TERMS_DECODED=''
    [[ -n $encoded && $encoded != *[^0123456789abcdef]* && $((${#encoded} % 2)) = 0 ]] || return 2
    while [[ -n $encoded ]]; do
        pair=${encoded[1,2]}; encoded=${encoded[3,-1]}
        [[ $pair != 00 ]] || return 2
        escapes+="\\x$pair"
    done
    printf -v TERMS_DECODED '%b' "$escapes"
}
_terms_track() {
    local rc=$?
    if [[ ${_TERMS_TRACKING:-0} = 1 && ${_TERMS_LAST_PWD-} != $PWD ]]; then
        if "$_TERMS_BIN" jump add "$PWD" >/dev/null; then _TERMS_LAST_PWD=$PWD; fi
    fi
    return $rc
}
_terms_jump() {
    local interactive=$1 encoded rc
    shift
    if [[ $interactive = no ]] && { (( $# == 0 )) || { (( $# == 1 )) && [[ -d $1 ]]; }; }; then
        if (( $# == 0 )); then builtin cd -- "$HOME" || return; else builtin cd -- "$1" || return; fi
    else
        if [[ $interactive = yes ]]; then encoded=$("$_TERMS_BIN" jump query --interactive --hex -- "$@")
        else encoded=$("$_TERMS_BIN" jump query --hex -- "$@"); fi
        rc=$?; (( rc == 0 )) || return $rc
        _terms_decode "$encoded" || return 2
        builtin cd -- "$TERMS_DECODED" || return
    fi
    _terms_track
}
_terms_complete() {
    local encoded line
    local -a paths
    encoded=$("$_TERMS_BIN" jump list --hex) || return
    for line in "${(@f)encoded}"; do
        _terms_decode "$line" || continue
        paths+=("$TERMS_DECODED")
    done
    compadd -f -- "${paths[@]}"
}
_terms_file_hex() {
    local record i total=0 LC_ALL=C
    "$_TERMS_BIN" search --files --null | while IFS= read -r -d '' record; do
        (( total += ${#record} ))
        (( total <= 1048576 )) || return 2
        for ((i=1; i<=${#record}; i++)); do printf '%02x' "'${record[i]}"; done
        printf '\n'
    done
}
_terms_insert() {
    local encoded candidates line result=''
    candidates=$(setopt pipefail; _terms_file_hex) || return
    encoded=$(unsetopt pipefail; for line in "${(@f)candidates}"; do
        [[ -n $line ]] || continue
        _terms_decode "$line" || exit 2
        printf '%s\0' "$TERMS_DECODED"
    done | "$_TERMS_BIN" pick --read0 --hex) || return
    for line in "${(@f)encoded}"; do
        _terms_decode "$line" || return 2
        result+="${(q)TERMS_DECODED} "
    done
    LBUFFER+=$result
}
_terms_change_dir() { tji; zle reset-prompt; }
terms_enable() {
    local requested=${1-}
    [[ -z $requested || $requested = --track ]] || return 2
    if [[ ${_TERMS_ENABLED:-0} != 1 ]]; then
        _TERMS_BIN=${TERMS_BIN:-${commands[terms]-}}
        [[ $_TERMS_BIN = /* && -x $_TERMS_BIN ]] || { print -u2 'terms: set TERMS_BIN to an absolute executable path'; return 2; }
        (( ${+functions[tj]} || ${+functions[tji]} )) && { print -u2 'terms: tj or tji already exists'; return 2; }
        tj() { _terms_jump no "$@"; }
        tji() { _terms_jump yes "$@"; }
        # compdef is provided by the user's initialized completion system.
        (( ${+functions[compdef]} )) && compdef _terms_complete tj tji
        _TERMS_TJ_DEF=$functions[tj]
        _TERMS_TJI_DEF=$functions[tji]
        # Short names for the other commands. An occupied name is skipped, never replaced.
        local name sub
        typeset -gA _TERMS_SHORT
        _TERMS_SHORT=()
        for name sub in t '' tl ls ts search tq json tp pick th history tg git; do
            if (( ${+aliases[$name]} || ${+functions[$name]} || ${+builtins[$name]} || ${+reswords[(r)$name]} || ${+commands[$name]} )); then
                print -u2 "terms: $name already exists; not defined"
                continue
            fi
            functions[$name]="\"\$_TERMS_BIN\" $sub \"\$@\""
            _TERMS_SHORT[$name]=$functions[$name]
        done
        _TERMS_ENABLED=1
    fi
    if [[ $requested = --track && ${_TERMS_TRACKING:-0} != 1 ]]; then
        typeset -ga chpwd_functions
        chpwd_functions=(${chpwd_functions:#_terms_track} _terms_track)
        _TERMS_TRACKING=1
        _terms_track
    fi
}
terms_bindings_enable() {
    [[ ${_TERMS_ENABLED:-0} = 1 ]] || return 2
    [[ ${_TERMS_BINDINGS:-0} = 1 ]] && return 0
    local current
    current=$(bindkey '^T')
    [[ $current = *undefined-key* ]] || { print -u2 'terms: Ctrl-T is occupied'; return 2; }
    current=$(bindkey '^[c')
    [[ $current = *undefined-key* ]] || { print -u2 'terms: Alt-C is occupied'; return 2; }
    zle -N _terms_insert
    zle -N _terms_change_dir
    bindkey '^T' _terms_insert
    bindkey '^[c' _terms_change_dir
    _TERMS_BINDINGS=1
}
terms_disable() {
    chpwd_functions=(${chpwd_functions:#_terms_track})
    [[ $(bindkey '^T') = *'_terms_insert' ]] && bindkey -r '^T'
    [[ $(bindkey '^[c') = *'_terms_change_dir' ]] && bindkey -r '^[c'
    if (( ${+functions[compdef]} )); then
        [[ ${_comps[tj]-} = _terms_complete ]] && compdef -d tj
        [[ ${_comps[tji]-} = _terms_complete ]] && compdef -d tji
    fi
    [[ ${functions[tj]-} != ${_TERMS_TJ_DEF-} ]] || unfunction tj
    [[ ${functions[tji]-} != ${_TERMS_TJI_DEF-} ]] || unfunction tji
    local name
    for name in ${(k)_TERMS_SHORT}; do
        [[ ${functions[$name]-} != ${_TERMS_SHORT[$name]} ]] || unfunction $name
    done
    unset _TERMS_SHORT
    unset _TERMS_ENABLED _TERMS_TRACKING _TERMS_LAST_PWD _TERMS_BINDINGS
}
