#!/usr/bin/perl
# Only SwiftDriver under a perf lease uses these launch adapters. The frozen
# dev launcher otherwise hands the app to LaunchServices and double-forks its
# mock; neither can be registered reliably by discovering its PID afterwards.
use strict;
use warnings;
use Cwd qw(abs_path);
use File::Basename qw(basename dirname);
use File::Temp qw(tempdir);
use Fcntl qw(:flock);
use POSIX qw(setsid);

my $self = abs_path($0);
my $gate = dirname($self) . '/perf-children.pl';
my $bin = dirname($self) . '/perf-swift-bin';
my $mode = basename($0);

if ($mode eq 'node') {
    my $real = $ENV{THREADBEAR_PERF_SWIFT_NODE} // die "perf-swift: no Node\n";
    my $instance = $ENV{THREADBEAR_PERF_SWIFT_INSTANCE} // die "perf-swift: no instance\n";
    my $registry = $ENV{THREADBEAR_PERF_CHILDREN} // die "perf-swift: no registry\n";
    my $mock = $ENV{THREADBEAR_PERF_SWIFT_MOCK} // die "perf-swift: no mock\n";
    my $owned = @ARGV && $ARGV[0] eq $mock;
    my $data = 0;
    for my $i (0 .. $#ARGV - 1) {
        $data = 1 if $ARGV[$i] eq '--data-dir' && $ARGV[$i + 1] eq "$instance/data";
    }
    # Non-mock Node calls keep their ordinary behaviour. The mock's setsid
    # process must exec the gate, retaining the PID the launcher recorded.
    if ($owned && $data) {
        exec $^X, $gate, '--child', $registry, $real, @ARGV;
        die "perf-swift: mock gate exec: $!\n";
    }
    die "perf-swift: mock belongs to a different instance\n" if $owned;
    exec {$real} $real, @ARGV;
    die "perf-swift: Node exec: $!\n";
}

if ($mode eq 'open') {
    my $instance = $ENV{THREADBEAR_PERF_SWIFT_INSTANCE} // die "perf-swift: no instance\n";
    my $registry = $ENV{THREADBEAR_PERF_CHILDREN} // die "perf-swift: no registry\n";
    my @original = @ARGV;
    my ($bundle, $out, $err);
    my (%env, @args);
    my $unknown = 0;
    while (@ARGV) {
        my $arg = shift @ARGV;
        if ($arg eq '--args') { @args = @ARGV; last; }
        next if $arg eq '-g' || $arg eq '-n';
        if ($arg eq '-a') { $bundle = shift @ARGV; }
        elsif ($arg eq '--stdout') { $out = shift @ARGV; }
        elsif ($arg eq '--stderr') { $err = shift @ARGV; }
        elsif ($arg eq '--env') {
            my ($key, $value) = split(/=/, shift(@ARGV) // '', 2);
            die "perf-swift: invalid environment argument\n" unless length($key) && defined($value);
            $env{$key} = $value;
        } else { $unknown = 1; }
    }
    # Never take ownership of another app or a URL the script may open.
    if (!defined($bundle) || $bundle ne "$instance/Threadbear.app") {
        exec '/usr/bin/open', @original;
        die "perf-swift: open exec: $!\n";
    }
    die "perf-swift: unsupported private app launch\n" if $unknown || !defined($out) || !defined($err);
    defined(my $pid = fork()) or die "perf-swift: fork: $!\n";
    exit 0 if $pid;
    setsid() >= 0 or die "perf-swift: setsid: $!\n";
    open(STDIN, '<', '/dev/null') or die "perf-swift: stdin: $!\n";
    open(STDOUT, '>', $out) or die "perf-swift: stdout: $!\n";
    open(STDERR, '>', $err) or die "perf-swift: stderr: $!\n";
    @ENV{keys %env} = values %env;
    # Even a fork delayed until after worker death must pass the gone gate
    # before the app can run. No asynchronous LaunchServices request remains.
    exec $^X, $gate, '--child', $registry,
        "$bundle/Contents/MacOS/Threadbear", @args;
    die "perf-swift: app exec: $!\n";
}

shift(@ARGV) eq '--run' or die "perf-swift: expected --run\n";
my ($registry, $node, $instance, $script) = splice @ARGV, 0, 4;
defined($script) or die "perf-swift: missing run arguments\n";
# Publish this exact instance and its adapters under the same barrier as all
# child registrations, before the start/rebuild script can fork anything.
open(my $state, '<', "$registry/state") or exit 129;
flock($state, LOCK_SH) or die "perf-swift: registration lock: $!\n";
exit 129 if -e "$registry/gone";
my $record = tempdir('swift-XXXXXXXX', DIR => $registry);
# Diagnostic ownership, written before launch; the PID records drive cleanup.
open(my $owner, '>', "$record/instance") or die "perf-swift: owner: $!\n";
print {$owner} $instance;
close $owner or die "perf-swift: owner record: $!\n";
# These adapters live in the checkout. A path inside the registry disappears
# on retirement and makes bash fall through to the real system open instead.
die "perf-swift: missing launch adapters\n" unless -x "$bin/open" && -x "$bin/node";
close $state;
$ENV{THREADBEAR_PERF_CHILDREN} = $registry;
$ENV{THREADBEAR_PERF_SWIFT_NODE} = $node;
$ENV{THREADBEAR_PERF_SWIFT_INSTANCE} = $instance;
$ENV{THREADBEAR_PERF_SWIFT_MOCK} = dirname($script) . '/mock-runner/server.mjs';
$ENV{THREADBEAR_NODE} = "$bin/node";
$ENV{PATH} = "$bin:$ENV{PATH}";
setpgrp(0, 0) or die "perf-swift: process group: $!\n";
exec $^X, $gate, '--child', $registry, $script, @ARGV;
die "perf-swift: launcher exec: $!\n";
