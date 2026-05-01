#!/bin/sh
# -*- coding: utf-8 -*-

basedir="$(realpath "$0" | xargs dirname)"

. "$basedir/scripts/lib.sh"

install_entry_checks()
{
    [ -d "$target" ] || die "codepal is not built! Run ./build.sh"
    [ "$(id -u)" = "0" ] || die "Must be root to install codepal."
}

install_dirs()
{
    do_install \
        -o root -g root -m 0755 \
        -d /opt/codepal/bin

    do_install \
        -o root -g root -m 0755 \
        -d /opt/codepal/etc

    do_install \
        -o root -g root -m 0755 \
        -d /opt/codepal/etc/codepal
}

install_codepal()
{
    do_install \
        -o root -g root -m 0755 \
        "$target/codepal" \
        /opt/codepal/bin/
}

release="release"
while [ $# -ge 1 ]; do
    case "$1" in
        --debug|-d)
            release="debug"
            ;;
        --release|-r)
            release="release"
            ;;
        *)
            die "Invalid option: $1"
            ;;
    esac
    shift
done
target="$basedir/target/$release"

install_entry_checks
install_dirs
install_codepal
