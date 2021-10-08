# QEMU D-Bus display experiment

## Introduction

*WIP* Rust crates to interact with a ``-display dbus`` QEMU. The development
branch is currently: https://gitlab.com/marcandre.lureau/qemu/-/tree/dbus

See also the QEMU mailing list for progress.

Most dependencies are released, but notably zbus 2.0 is still in beta. RDW and
VTE4 widgets are also unreleased at this stage.

## Features

Depending on what the VM exposes & supports, various interfaces are implemented:

 - display, with optional DMABUF sharing
 - display resize
 - keyboard & mouse
 - serial terminals
 - QMP/HMP monitors
 - audio playback & recording
 - USB device redirection
 - clipboard sharing

## Project organization

### qemu-display

This crate provides simple D-Bus interfaces through zbus, and some basic abstractions.

### qemu-rdw

This crate aims to provide Gtk+ 4 widget for a QEMU display, as well as
dialogs/widgets for USB redirection and other options or features.

Currently it's a demo app (run by default with `cargo run`). 

It is also based on a *WIP* crate "RDW" (*Remote display/desktop widget*) to
provide a base widget for various remote display solutions (VNC, RDP, Spice
etc).

### qemu-vnc

A simple VNC server implementation.

### qemu-vte

A standalone VTE/Gtk+ 4 client, which should eventually be a consumable crate or
integrated with qemu-rdw.

## Build requirements

To build this project, you will need several system libraries. Here is the
current list of build dependencies on Fedora:

```sh
$ sudo dnf install cargo gcc usbredir-devel wayland-devel libxkbcommon-devel glib2-devel gtk4-devel gstreamer1-devel gstreamer1-plugins-base-devel
```
