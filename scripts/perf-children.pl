#!/usr/bin/perl
# Detached apps/mock runners must not outlive a failed measurement worker.
# --child registers its own process group before exec; --reap runs the hangup
# guard and stops registered groups before returning to the lock holder.
use strict;
use warnings;
use Fcntl qw(:flock :DEFAULT);
use File::Path qw(remove_tree);
use File::Basename qw(dirname);
use FindBin;
use POSIX qw(dup2 WNOHANG);
use IPC::Open3;
use IO::Select;
use Symbol qw(gensym);
use Time::HiRes qw(clock_gettime CLOCK_MONOTONIC);
$SIG{CHLD} = 'DEFAULT';

my ($mode, $dir) = splice @ARGV, 0, 2;
my $log = dirname($dir) . '/perf-cleanup.log';
my $last_note = time;
sub note_if_stuck {
    return if time - $last_note < 30;
    $last_note = time;
    if (sysopen(my $out, $log, O_WRONLY | O_CREAT | O_APPEND, 0600)) {
        print {$out} scalar(gmtime) . " UTC: $_[0]\n";
        close $out;
    }
}
sub started {
    my ($pid) = @_;
    local $ENV{LC_ALL} = 'C';
    local $ENV{TZ} = 'UTC';
    open(my $ps, '-|', 'ps', '-p', $pid, '-o', 'lstart=') or return '';
    my $time = do { local $/; <$ps> } // '';
    return '' unless close $ps;
    $time =~ s/^\s+|\s+$//g;
    return $time;
}
# One process table for ancestry; descriptor ownership is checked separately.
sub snapshot {
    for (;;) {
        local $ENV{LC_ALL} = 'C';
        local $ENV{TZ} = 'UTC';
        if (open(my $ps, '-|', 'ps', '-axo', 'pid=,ppid=,pgid=,uid=,stat=,lstart=')) {
            my %rows;
            while (<$ps>) {
                next unless /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s+(.+?)\s*$/;
                $rows{$1} = { parent => $2, group => $3, uid => $4, state => $5, born => $6 };
            }
            return %rows if close($ps) && %rows;
        }
        note_if_stuck('ps unavailable while finding performance children; locks retained');
        select(undef, undef, undef, 0.2);
    }
}
my %probe_children;
sub carriers {
    my ($proof, $rows) = @_;
    # This descriptor is a lease capability, not a path/word in argv. Its
    # inode stays reserved until all copies close, even after retirement.
    if ($^O eq 'linux') {
        my %found;
        opendir(my $proc, '/proc') or return (0, {});
        while (my $pid = readdir($proc)) {
            next unless $pid =~ /^\d+$/;
            my @process = stat("/proc/$pid");
            next unless @process && $process[4] == $<;
            my @s = stat("/proc/$pid/fd/197");
            $found{$pid} = 1 if @s && $s[0] == $proof->[0] && $s[1] == $proof->[1];
        }
        closedir $proc;
        return (1, \%found);
    }
    $^O eq 'darwin' or die "perf-children: unsupported cleanup host\n";
    for my $pid (keys %probe_children) {
        my $done = waitpid($pid, WNOHANG);
        if ($done != 0) { delete $probe_children{$pid}; }
        else { kill 'KILL', $pid; } # our unreaped direct child cannot be reused
    }
    return (0, {}) if %probe_children;
    {
        my ($in, $out, $err) = (undef, undef, gensym());
        my $pid = eval { open3($in, $out, $err, '/usr/sbin/lsof', '-nPwb', '-a',
            '-u', "$<", '-d', '197', '-FpfDi') };
        if ($pid) {
            close $in;
            my $input = IO::Select->new($out, $err);
            my ($data, $errors) = ('', '');
            my $output_fd = fileno($out);
            my $deadline = clock_gettime(CLOCK_MONOTONIC) + 2;
            my $timed_out = 0;
            while ($input->count) {
                my $left = $deadline - clock_gettime(CLOCK_MONOTONIC);
                if ($left <= 0) { kill 'KILL', $pid; $errors .= 'timeout'; $timed_out = 1; last; }
                for my $fh ($input->can_read($left)) {
                    my $n = sysread($fh, my $chunk, 8192);
                    if (defined($n) && $n == 0) { $input->remove($fh); close $fh; next; }
                    next unless defined $n;
                    if (fileno($fh) == $output_fd) { $data .= $chunk; }
                    else { $errors .= $chunk; }
                }
            }
            close $_ for $input->handles;
            my $done = waitpid($pid, WNOHANG);
            # EOF on the output pipes is not proof that the probe exited.
            while ($done == 0 && !$timed_out && clock_gettime(CLOCK_MONOTONIC) < $deadline) {
                select(undef, undef, undef, 0.01);
                $done = waitpid($pid, WNOHANG);
            }
            if ($done == 0) { kill 'KILL', $pid; $probe_children{$pid} = 1; return (0, {}); }
            my $status = $?;
            # lsof exits 1 without diagnostics when no descriptor matches.
            if (($status == 0 || $status == 256) && !length($errors)) {
                my (%found, $process, $device);
                for my $line (split /\n/, $data) {
                    if ($line =~ /^p(\d+)$/) { $process = $1; $device = undef; }
                    elsif ($line =~ /^D(0x[0-9a-f]+)$/i) { $device = hex($1); }
                    elsif ($line =~ /^i(\d+)$/ && defined($device) &&
                        $device == $proof->[0] && $1 == $proof->[1]) {
                        $found{$process} = 1;
                    }
                }
                return (1, \%found);
            }
        }
        note_if_stuck('descriptor probe unavailable while finding performance children; locks retained');
        return (0, {});
    }
}
sub held_groups {
    my ($rows, $anchors, $groups) = @_;
    return map { $_ => $anchors->{$_} } grep {
        my $a = $anchors->{$_};
        my $p = $rows->{$a->{pid}};
        my $leader = $rows->{$_};
        $p && $a->{leader} eq $groups->{$_} && $a->{pid} != $_ && $p->{uid} == $< && $p->{state} !~ /^Z/ &&
            $p->{group} == $_ && $p->{born} eq $a->{born} &&
            (!$leader || $leader->{born} eq $groups->{$_});
    } keys %$anchors;
}
sub anchor_matches {
    my ($anchor, $group) = @_;
    local $ENV{LC_ALL} = 'C';
    local $ENV{TZ} = 'UTC';
    open(my $ps, '-|', 'ps', '-p', $anchor->{pid}, '-o', 'pgid=,uid=,stat=,lstart=') or return 0;
    my $row = <$ps> // '';
    return 0 unless close $ps;
    return $row =~ /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(.+?)\s*$/ &&
        $1 == $group && $2 == $< && $3 !~ /^Z/ && $4 eq $anchor->{born};
}
sub owned_processes {
    my ($rows, $groups, $known, $marked, $held) = @_;
    my %tree;
    for my $pid (keys %$rows) {
        next if $pid == $$;
        my $p = $rows->{$pid};
        $tree{$pid} = 1 if $marked->{$pid} || exists($held->{$p->{group}}) ||
            (exists($groups->{$pid}) && $p->{group} == $pid && $groups->{$pid} eq $p->{born}) ||
            (exists($known->{$pid}) && $known->{$pid} eq $p->{born});
    }
    my $added = 1;
    while ($added) {
        $added = 0;
        for my $pid (keys %$rows) {
            next if $pid == $$;
            next if $tree{$pid};
            my $parent = $rows->{$pid}->{parent};
            next unless $tree{$parent} && exists($rows->{$parent});
            # A failed/old ps row must not claim a reused parent's subtree.
            next unless started($parent) eq $rows->{$parent}->{born};
            $tree{$pid} = 1;
            $added = 1;
        }
    }
    return map { $_ => $rows->{$_}->{born} }
        grep { $_ != $$ && $rows->{$_}->{uid} == $< && $rows->{$_}->{state} !~ /^Z/ } keys %tree;
}

sub anchor_alone {
    my ($group) = @_;
    my $query = open(my $ps, '-|', 'ps', '-axo', 'pid=,pgid=,stat=') or return 0;
    my ($self, $others) = (0, 0);
    while (<$ps>) {
        next unless /^\s*(\d+)\s+(\d+)\s+(\S+)/ && $2 == $group && $3 !~ /^Z/;
        $self = 1 if $1 == $$;
        $others = 1 if $1 != $$ && $1 != $query;
    }
    return 0 unless close $ps;
    return $self && !$others;
}
sub retire_anchor {
    my ($dir, $group, $identity) = @_;
    # Use a private open description: fd197 must never acquire a flock.
    open(my $gate, '<', "$dir/state") or return 0;
    my @s = stat($gate);
    return 0 unless @s && $s[0] == $identity->[0] && $s[1] == $identity->[1];
    return 0 unless flock($gate, LOCK_SH | LOCK_NB);
    return 0 if !-d $dir || -e "$dir/gone";
    # Keep numeric records stable for callers that count registrations. The
    # tombstone only retires the leader generation recorded by this anchor.
    return rename("$dir/anchor-$group", "$dir/retired-$group");
}
sub start_anchor {
    my ($dir, $state, $leader_born) = @_;
    seek($state, 0, 0) or die "perf-children: guardian identity: $!\n";
    chomp(my $guardian = <$state> // '');
    chomp(my $born = <$state> // '');
    # Old/direct --child callers may have an empty registry state file.
    return unless $guardian =~ /^\d+$/ && length($born);
    my $group = $$;
    if (open(my $record, '<', "$dir/anchor-$group")) {
        chomp(my $pid = <$record> // '');
        chomp(my $time = <$record> // '');
        chomp(my $leader = <$record> // '');
        close $record;
        return if $pid =~ /^\d+$/ && $leader eq $leader_born &&
            anchor_matches({ pid => $pid, born => $time }, $group);
    }
    my @s = stat($state);
    exit 129 unless @s;
    my $identity = [@s[0, 1]];
    pipe(my $ready_r, my $ready_w) or die "perf-children: anchor pipe: $!\n";
    my $intermediate = fork() // die "perf-children: anchor fork: $!\n";
    if ($intermediate) {
        close $ready_w;
        my $n = sysread($ready_r, my $ready, 1);
        close $ready_r;
        my $done = waitpid($intermediate, 0);
        my $status = $?;
        exit 129 unless $done == $intermediate && $status == 0 && defined($n) && $n == 1 && $ready eq '1';
        return;
    }
    close $ready_r;
    # It must not be in the guard's descendant tree, and the actual command
    # must inherit neither an unexpected child nor a zombie to wait for.
    my $anchor = fork();
    exit 129 unless defined $anchor;
    exit 0 if $anchor;
    $SIG{TERM} = $SIG{HUP} = $SIG{INT} = $SIG{PIPE} = 'IGNORE';
    open(STDIN, '<', '/dev/null');
    open(STDOUT, '>', '/dev/null');
    open(STDERR, '>', '/dev/null');
    # Never hold a command's output pipes, host locks, or an app-looking argv.
    POSIX::close(198); POSIX::close(199);
    $0 = "threadbear-perf-anchor $group";
    @ARGV = ();
    my $time = started($$);
    exit 129 unless length($time);
    open(my $record, '>', "$dir/anchor-$group") or exit 129;
    print {$record} "$$\n$time\n$leader_born";
    close $record or exit 129;
    # The anchor keeps the original PGID reserved, even if Node drops fd197
    # in its children and then dies. It never leaves this group or execs.
    close $state;
    print {$ready_w} '1';
    close $ready_w;
    my $check_at = time + 30;
    my $group_check_at = 0;
    while (kill(0, $guardian)) {
        sleep 1;
        if (!kill(0, $group) && time >= $group_check_at) {
            if (anchor_alone($group)) {
                select(undef, undef, undef, 0.15);
                last if anchor_alone($group) && retire_anchor($dir, $group, $identity);
            }
            # When retirement already owns EX, stay alive to pin the group
            # until its cached registration has been consumed by cleanup.
            $group_check_at = time + 2;
        }
        if (time >= $check_at) {
            my $now = started($guardian);
            last if length($now) && $now ne $born;
            $check_at = time + 30;
        }
    }
    exit 0;
}
sub kill_owned {
    my ($owned, $rows, $groups, $anchors) = @_;
    $anchors //= {};
    # Leaves first. No inferred group signals: a child's group may include
    # processes that did not come from this lease.
    my %left = %$owned;
    while (%left) {
        my %parents = map { $rows->{$_}->{parent} => 1 } keys %left;
        my @leaves = grep { !$parents{$_} } keys %left;
        @leaves = keys %left unless @leaves;
        for my $pid (@leaves) {
            kill 'KILL', $pid if !$anchors->{$pid} && !exists($groups->{$rows->{$pid}->{group}}) &&
                started($pid) eq $left{$pid};
            delete $left{$pid};
        }
    }
}
sub collect_owned {
    my ($proof, $groups, $anchors, $known) = @_;
    my %rows = snapshot();
    my ($ok, $marked) = carriers($proof, \%rows);
    my %held = held_groups(\%rows, $anchors, $groups);
    my %owned = owned_processes(\%rows, $groups, $known, $marked, \%held);
    my $raced = grep { !exists($rows{$_}) } keys %$marked;
    my %active = map { $_->{group} => 1 }
        grep { $_->{uid} == $< && $_->{state} !~ /^Z/ } values %rows;
    my @uncertain = grep {
        my $leader = $rows{$_};
        $active{$_} && !exists($held{$_}) && (!$leader ||
            ($leader->{born} eq $groups->{$_} && ($leader->{state} =~ /^Z/ || $leader->{uid} != $<)));
    } keys %$groups;
    # If an anchor was killed alone, don't guess whether its leaderless group
    # is still ours or was reused. Neither signal it nor declare it quiet.
    note_if_stuck('unproven registered groups ' . join(',', @uncertain) . '; locks retained') if @uncertain;
    return (\%rows, \%owned, \%held, $ok && $marked->{$$} && !$raced && !@uncertain);
}

if ($mode eq '--child') {
    # Shared while registering; cleanup marks the run gone under an exclusive
    # lock. A late child either registers before that or exits before exec.
    open(my $state, '<', "$dir/state") or exit 129;
    flock($state, LOCK_SH) or die "perf-children: registration lock: $!\n";
    exit 129 if -e "$dir/gone";
    die "perf-children: child must lead its own group\n" unless getpgrp(0) == $$;
    my $time = started($$);
    die "perf-children: can't identify child\n" unless length $time;
    unlink("$dir/retired-$$") or exit 129 if -e "$dir/retired-$$";
    open(my $record, '>', "$dir/$$") or exit 129;
    print {$record} $time;
    close $record or die "perf-children: record: $!\n";
    # A separate, unlocked open file description: keeping the registration's
    # flock alive here would deadlock retirement. dup2 clears close-on-exec.
    open(my $proof, '<', "$dir/state") or exit 129;
    my $source = fileno($proof);
    dup2($source, 197) >= 0 or die "perf-children: proof descriptor: $!\n";
    if ($source != 197) { close $proof; open($proof, '<&=', 197) or die $!; }
    fcntl($proof, F_SETFD, 0) or die "perf-children: inherit proof: $!\n";
    start_anchor($dir, $state, $time);
    close $state;
    exec { $ARGV[0] } @ARGV or die "perf-children: exec: $!\n";
}
$mode eq '--reap' or die "perf-children: unknown mode\n";
$SIG{PIPE} = 'IGNORE';
# Written before any workload starts; readers use it only to make their
# anchor exit if this reaper dies. The inode and registration gate stay put.
open(my $owner, '>', "$dir/state") or die "perf-children: guardian: $!\n";
my $born = started($$);
die "perf-children: can't identify guardian\n" unless length($born);
print {$owner} "$$\n$born\n";
close $owner or die "perf-children: guardian record: $!\n";
my $guard = fork() // die "perf-children: fork: $!\n";
if (!$guard) {
    $SIG{PIPE} = 'DEFAULT';
    exec '/bin/bash', "$FindBin::Bin/hangup-guard.sh", @ARGV;
    die "perf-children: guard: $!\n";
}
waitpid($guard, 0);
my $status = $?;
# From here cleanup must survive the caller's stdout/stderr having gone.
open(STDERR, '>', '/dev/null');
open(my $state, '<', "$dir/state") or die "perf-children: no registry\n";
flock($state, LOCK_EX) or die "perf-children: cleanup lock\n";
open(my $gone, '>', "$dir/gone") or die "perf-children: mark gone\n";
close $gone;
# Retire the public name atomically before removing anything. A late child
# holding the old state fd can never recreate a record after the scan.
my $retired = "$dir.reaping";
my $renamed = rename($dir, $retired);
$dir = $retired if $renamed;
opendir(my $entries, $dir) or die "perf-children: registry\n";
my (%groups, %anchors, %retired_groups);
while (my $pid = readdir($entries)) {
    if ($pid =~ /^(anchor|retired)-(\d+)$/) {
        my ($kind, $group) = ($1, $2);
        open(my $record, '<', "$dir/$pid") or next;
        chomp(my $anchor = <$record> // '');
        chomp(my $time = <$record> // '');
        chomp(my $leader = <$record> // '');
        close $record;
        if ($anchor =~ /^\d+$/ && length($time) && length($leader)) {
            if ($kind eq 'retired') { $retired_groups{$group} = $leader; }
            else { $anchors{$group} = { pid => $anchor, born => $time, leader => $leader }; }
        }
        next;
    }
    next unless $pid =~ /^\d+$/;
    open(my $record, '<', "$dir/$pid") or next;
    my $time = do { local $/; <$record> } // '';
    close $record;
    $groups{$pid} = $time;
}
closedir $entries;
delete $groups{$_} for grep { exists($groups{$_}) && $groups{$_} eq $retired_groups{$_} } keys %retired_groups;
delete $anchors{$_} for grep { !exists($groups{$_}) } keys %anchors;

# The file identity is stable across rename, and cannot be reused while an
# inherited descriptor remains open. It carries no host-lock ownership.
my @proof_stat;
while (!(@proof_stat = stat($state))) {
    note_if_stuck('registry identity unavailable; locks retained');
    select(undef, undef, undef, 0.2);
}
my $marker = [@proof_stat[0, 1]];
my $control;
for (;;) {
    if (open($control, '<', "$dir/state")) {
        my @s = stat($control);
        if (@s && $s[0] == $marker->[0] && $s[1] == $marker->[1]) {
            my $source = fileno($control);
            if (dup2($source, 197) >= 0) {
                my $opened = $source == 197;
                if (!$opened) { close $control; $opened = open($control, '<&=', 197); }
                last if $opened && fcntl($control, F_SETFD, FD_CLOEXEC);
            }
        }
        close $control if defined(fileno($control));
    }
    note_if_stuck('descriptor positive control unavailable; locks retained');
    select(undef, undef, undef, 0.2);
}
# This unlocked, close-on-exec copy is a positive control for every scan.
# Query children never inherit it, and it is excluded before ancestry walks.
close $state;
my %known;
my $quiet = 0;
my $deadline;
for (my $pass = 0; ; $pass++) {
    my ($rows, $owned, $held, $complete) = collect_owned($marker, \%groups, \%anchors, \%known);
    %known = %$owned;
    my %anchor_pids = map { $_->{pid} => 1 } values %$held;
    kill_owned(\%known, $rows, \%groups, \%anchor_pids);
    # Registered apps/mock runners still get their graceful TERM interval.
    if ($pass == 0) {
        for my $pid (keys %known) {
            kill 'TERM', $pid if !$anchor_pids{$pid} && exists($groups{$rows->{$pid}->{group}}) &&
                started($pid) eq $known{$pid};
        }
        $deadline = clock_gettime(CLOCK_MONOTONIC) + 5;
    }
    last unless grep { !$anchor_pids{$_} } keys %known;
    last if clock_gettime(CLOCK_MONOTONIC) >= $deadline;
    select(undef, undef, undef, 0.2);
}
# A live recorded anchor pins its original PGID, so the final group KILL
# catches forks even when Node closes fd197 in children. Never infer group
# ownership merely from a missing leader. Other sessions require proof or
# captured ancestry. Keep the locks until group KILLs are observed to stop.
my %ending;
for (;;) {
    my ($rows, $owned, $held, $complete) = collect_owned($marker, \%groups, \%anchors, \%known);
    %known = %$owned;
    my %anchor_pids = map { $_->{pid} => 1 } values %$held;
    kill_owned(\%known, $rows, \%groups, \%anchor_pids);
    for my $group (keys %$held) {
        my $anchor = $held->{$group};
        next unless anchor_matches($anchor, $group);
        $ending{$group} = $groups{$group} if kill('KILL', -$group);
    }
    kill_owned(\%known, $rows, {}, \%anchor_pids);
    for my $group (keys %ending) {
        my $leader = $rows->{$group};
        delete $ending{$group} if ($leader && $leader->{born} ne $ending{$group}) ||
            !grep { $_->{group} == $group && $_->{state} !~ /^Z/ } values %$rows;
    }
    $quiet = $complete && !%known && !%ending ? $quiet + 1 : 0;
    last if $quiet >= 2;
    note_if_stuck('still stopping performance children ' . join(',', keys %known) . '; locks retained');
    select(undef, undef, undef, 0.05);
}
remove_tree($dir) if $renamed;
exit($status & 127 ? 128 + ($status & 127) : $status >> 8);
