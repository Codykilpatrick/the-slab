# Bash completion for slab
# Install: source this file or add to ~/.bashrc

_slab_completions() {
    local cur prev words cword
    _init_completion || return

    local commands="chat run config models sessions test init completions"
    local global_opts="-m --model -c --config -v --verbose --no-stream -h --help -V --version"

    # Determine position in command
    local cmd=""
    for ((i=1; i < cword; i++)); do
        case "${words[i]}" in
            chat|run|config|models|sessions|test|init|completions)
                cmd="${words[i]}"
                break
                ;;
        esac
    done

    case "$cmd" in
        chat)
            case "$prev" in
                -s|--session)
                    # Dynamic session completion
                    local sessions
                    sessions=$(slab sessions --names-only 2>/dev/null)
                    COMPREPLY=($(compgen -W "$sessions" -- "$cur"))
                    return
                    ;;
            esac
            COMPREPLY=($(compgen -W "-C --continue -s --session $global_opts" -- "$cur"))
            ;;
        run)
            COMPREPLY=($(compgen -W "$global_opts" -- "$cur"))
            ;;
        config)
            COMPREPLY=($(compgen -W "--show --init --set $global_opts" -- "$cur"))
            ;;
        models)
            COMPREPLY=($(compgen -W "--names-only $global_opts" -- "$cur"))
            ;;
        sessions)
            COMPREPLY=($(compgen -W "--names-only $global_opts" -- "$cur"))
            ;;
        test)
            case "$prev" in
                --model)
                    # Dynamic model completion
                    local models
                    models=$(slab models --names-only 2>/dev/null)
                    COMPREPLY=($(compgen -W "$models" -- "$cur"))
                    return
                    ;;
            esac
            COMPREPLY=($(compgen -W "--filter --model $global_opts" -- "$cur"))
            ;;
        completions)
            COMPREPLY=($(compgen -W "bash zsh fish powershell" -- "$cur"))
            ;;
        init)
            COMPREPLY=($(compgen -W "$global_opts" -- "$cur"))
            ;;
        "")
            # No command yet - check for global options or commands
            case "$prev" in
                -m|--model)
                    # Dynamic model completion
                    local models
                    models=$(slab models --names-only 2>/dev/null)
                    COMPREPLY=($(compgen -W "$models" -- "$cur"))
                    return
                    ;;
                -c|--config)
                    # File completion
                    _filedir toml
                    return
                    ;;
            esac
            COMPREPLY=($(compgen -W "$commands $global_opts" -- "$cur"))
            ;;
    esac
}

complete -F _slab_completions slab
