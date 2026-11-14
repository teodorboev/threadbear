#!/usr/bin/perl
# One build or measurement per host. Both files stay locked until the hangup
# guard has stopped the command's tree, including Desk Runner's GUI child.
# Usage: perl scripts/perf-lock.pl <pidfile|-> <command...>
#        perl scripts/perf-lock.pl --desk <pidfile> <command...>
#        perl scripts/perf-lock.pl --try <pidfile|-> <command...>  (75 if busy)
use strict;
use warnings;
use Fcntl qw(:flock);
use Errno qw(EAGAIN EWOULDBLOCK EINTR);
use File::Path qw(make_path);
use FindBin;
use File::Spec;
use File::Temp qw(tempdir);
use IO::Select;
use POSIX qw(WNOHANG dup2);
use Sys::Hostname qw(hostname);
use Time::Local qw(timegm);

my ($desk, $try) = (0, 0);
while (@ARGV && ($ARGV[0] eq '--desk' || $ARGV[0] eq '--try')) {
    my $arg = shift @ARGV;
    if ($arg eq '--desk') { $desk = 1; } else { $try = 1; }
}
my $pidfile = shift(@ARGV) // die "perf-lock: missing pidfile or -\n";
@ARGV or die "perf-lock: missing command\n";
die "perf-lock: --desk needs a pidfile\n" if $desk && $pidfile eq '-';
$pidfile = File::Spec->rel2abs($pidfile) if $pidfile ne '-';
my $home = $ENV{HOME} // die "perf-lock: no HOME\n";
my $host = hostname();
my $owner = $ENV{THREADBEAR_PERF_LOCK_OWNER} // '';
# A marker only belongs to this host and home, and to a living lock holder.
# Desk Runner gets it explicitly below, since its service has its own env.
if ($owner =~ /^\Q$host\E:(\d+):\Q$home\E$/ && $1 > 0 && kill(0, $1)) {
    die "perf-lock: --desk starts a remote run; call its workload directly inside an existing run\n" if $desk;
    exec { $ARGV[0] } @ARGV or die "perf-lock: exec: $!\n";
}

my $cancelled = 0;
$SIG{TERM} = sub { $cancelled = 143 };
$SIG{HUP} = sub { $cancelled = 129 };
$SIG{INT} = sub { $cancelled = 130 };
# A dropped ssh can make even a diagnostic write fail. Cleanup must finish.
$SIG{PIPE} = 'IGNORE';
my $input = IO::Select->new(\*STDIN);
my $last_beat = 0;
sub keep_copy {
    return if time - $last_beat < 30;
    $last_beat = time;
    for my $name (qw(THREADBEAR_REMOTE_SOURCE THREADBEAR_REMOTE_TARGET THREADBEAR_REMOTE_TOOLS)) {
        my $dir = $ENV{$name} // next;
        next unless -d $dir;
        open(my $marker, '>>', "$dir/.in-use") or next;
        utime(undef, undef, "$dir/.in-use", $dir);
        close $marker;
    }
}
sub channel_gone {
    return 0 unless $input->can_read(0);
    my $n = sysread(STDIN, my $byte, 1);
    return defined($n) && $n == 0;
}
sub stopping {
    keep_copy();
    $cancelled ||= 129 if channel_gone();
    return $cancelled;
}

# Who holds the perf lock, for whoever is waiting. lsof can't say: every
# waiter has the file open too, and a waiter at 0% CPU looked like a holder
# that never started its run (14 Nov: a thread killed its own queued --desk
# run, three minutes after it had finally got the lock and started).
my $holder_file = "$home/.cache/threadbear-perf.lock.holder";
sub holder_text {
    open(my $in, '<', $holder_file) or return 'whoever holds it left no note (an older perf-lock.pl or lockf)';
    my ($pid, $since, $label) = map { my $l = $_ // ''; chomp $l; $l } (<$in>)[0 .. 2];
    close $in;
    return 'whoever holds it left no note' unless ($pid // '') =~ /^\d+$/ && ($since // '') =~ /^\d+$/;
    my $mins = int((time - $since) / 60);
    my $alive = kill(0, $pid) ? '' : '; its run has ended and its cleanup is stopping what it left';
    return "held by perf-lock pid $pid for ${mins}m, running: $label$alive";
}
sub write_holder {
    my $label = $ENV{THREADBEAR_PERF_LOCK_LABEL} // join(' ', @ARGV);
    $label =~ s/\s+/ /g;
    $label = substr($label, 0, 157) . '...' if length($label) > 160;
    my $tmp = "$holder_file.$$";
    open(my $out, '>', $tmp) or return;
    print {$out} "$$\n", time, "\n$label\n";
    close $out;
    rename($tmp, $holder_file) or unlink($tmp);
}
my $wrote_holder = 0;
# And how many runs are queued ahead: one file per waiter, named by its pid,
# holding when it started waiting. Only a living pid counts.
my $main_pid = $$;
my $waiting_dir = "$holder_file" =~ s/\.holder$/.waiting/r;
my $waiting_since;
# Updaters never join the queue, and must not jump ahead of a waiter whose
# blocking locker hasn't been scheduled yet. Stale notes don't hold it up.
sub defer_to_waiters {
    return unless $try && opendir(my $dir, $waiting_dir);
    my @pids = grep { /^\d+$/ && $_ > 0 && $_ != $$ && kill(0, $_) } readdir $dir;
    closedir $dir;
    for my $pid (@pids) {
        open(my $in, '<', "$waiting_dir/$pid") or next;
        my $since = <$in>; close $in;
        next unless defined($since) && $since =~ /^(\d+)\s*$/;
        $since = $1;
        # A killed waiter's note can outlast a reboot and pid reuse. A
        # process born after the note isn't its waiter. lstart works on both
        # macOS and Linux; use UTC and the C locale for one date format.
        local $ENV{LC_ALL} = 'C';
        local $ENV{TZ} = 'UTC';
        my $born = '';
        if (open(my $ps, '-|', 'ps', '-p', $pid, '-o', 'lstart=')) {
            $born = <$ps> // ''; close $ps;
        }
        if ($born =~ /^\s*\w+\s+(\w+)\s+(\d+)\s+(\d+):(\d+):(\d+)\s+(\d+)\s*$/) {
            my %month; @month{qw(Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec)} = (0 .. 11);
            my ($m, $d, $h, $min, $s, $y) = ($1, $2, $3, $4, $5, $6);
            my $at = eval { timegm($s, $min, $h, $d, $month{$m}, $y) };
            next if defined($at) && $at > $since;
        }
        # If ps couldn't say, leave the live waiter ahead rather than guess.
        next unless kill(0, $pid);
        print STDERR "Host has queued work; deferring the update build.\n";
        exit 75;
    }
}
sub start_waiting {
    make_path($waiting_dir);
    $waiting_since = time;
    open(my $out, '>', "$waiting_dir/$$") or return;
    print {$out} "$waiting_since\n";
    close $out;
}
sub stop_waiting { unlink("$waiting_dir/$main_pid") if defined $waiting_since; }
sub queued_ahead {
    return 0 unless defined $waiting_since && opendir(my $dir, $waiting_dir);
    my $ahead = 0;
    for my $pid (grep { /^\d+$/ && $_ != $$ } readdir $dir) {
        open(my $in, '<', "$waiting_dir/$pid") or next;
        my $since = <$in>; close $in;
        $ahead++ if defined($since) && $since =~ /^(\d+)$/ && $1 <= $waiting_since && kill(0, $pid);
    }
    closedir $dir;
    return $ahead;
}
sub queue_text {
    my $n = queued_ahead();
    return $n ? "; $n other run" . ($n == 1 ? '' : 's') . " waiting since before this one" : '';
}
END {
    # The locker and watcher forks run END too: only the supervisor tidies.
    stop_waiting() if $$ == $main_pid;
    # Only our own note: a later holder may already have written its own.
    if ($$ == $main_pid && $wrote_holder && open(my $in, '<', $holder_file)) {
        my $pid = <$in>; close $in;
        unlink($holder_file) if defined($pid) && $pid =~ /^(\d+)$/ && $1 == $$;
    }
}

sub wait_for_lock {
    my ($lock, $what, @held) = @_;
    pipe(my $ready_r, my $ready_w) or die "perf-lock: ready pipe: $!\n";
    pipe(my $live_r, my $live_w) or die "perf-lock: owner pipe: $!\n";
    my $locker = fork() // die "perf-lock: locker fork: $!\n";
    if (!$locker) {
        close $ready_r; close $live_r; close $live_w;
        $SIG{TERM} = $SIG{INT} = $SIG{HUP} = $SIG{PIPE} = 'DEFAULT';
        flock($lock, LOCK_EX) or die "perf-lock: blocking lock: $!\n";
        syswrite($ready_w, '1');
        exit 0;
    }
    close $ready_w;
    my $watcher = fork() // die "perf-lock: lock watcher fork: $!\n";
    if (!$watcher) {
        close $live_w;
        close $_ for ($lock, @held);
        # No lock fds in this watcher. If even KILL takes the owner while it
        # waits, stop its blocking locker; readiness also means it can leave.
        my $ready = IO::Select->new($ready_r);
        my $live = IO::Select->new($live_r);
        for (;;) {
            exit 0 if $ready->can_read(0);
            if ($live->can_read(0.1)) {
                kill 'TERM', $locker unless $ready->can_read(0);
                exit 0;
            }
        }
    }
    close $live_r;
    my $ready = IO::Select->new($ready_r);
    my $next_note = time + 300;
    while (!$ready->can_read(0.1)) {
        last if stopping();
        next unless $what && time >= $next_note;
        $next_note = time + 300;
        print STDERR "Still waiting for $what on $host: " . holder_text() . queue_text() . "\n";
    }
    close $live_w if $cancelled;
    my $count = $cancelled ? 0 : sysread($ready_r, my $byte, 1);
    close $ready_r;
    waitpid($locker, 0);
    my $status = $?;
    close $live_w unless $cancelled;
    waitpid($watcher, 0);
    exit $cancelled if $cancelled;
    die "perf-lock: blocking locker failed\n" unless $count && $status == 0;
    # flock is on the shared open description, so this parent's fd keeps it
    # held when the locker exits. It never drops its place in the kernel queue.
}

make_path("$home/.cache/threadbear-check");
defer_to_waiters();
my @locks;
for my $spec (["$home/.cache/threadbear-perf.lock", 'another build or measurement'],
              ["$home/.cache/threadbear-check/lock", 'another check']) {
    my $said = 0;
    # Keep the lock we actually acquired, including while waiting for the next
    # one. A free probe followed by a build races a check starting after it.
    for (;;) {
        open(my $lock, '>>', $spec->[0]) or die "perf-lock: $spec->[0]: $!\n";
        my $locked = flock($lock, LOCK_EX | LOCK_NB);
        while (!$locked) {
            die "perf-lock: lock $spec->[0]: $!\n" unless $! == EAGAIN || $! == EWOULDBLOCK || $! == EINTR;
            exit $cancelled if stopping();
            if ($try) {
                print STDERR "Host is busy ($spec->[1]); deferring the update build.\n";
                exit 75;
            }
            my $perf = $spec->[0] eq "$home/.cache/threadbear-perf.lock";
            start_waiting() if $perf && !defined $waiting_since;
            print STDERR "Waiting for $spec->[1] on $host ($spec->[0])" . ($perf ? ', ' . holder_text() . queue_text() : '') . "...\n" unless $said++;
            wait_for_lock($lock, $perf ? $spec->[1] : '', @locks);
            $locked = 1;
        }
        exit $cancelled if stopping();
        # Old lockf calls without -k unlink on exit. A waiter can then own a
        # deleted inode while a new caller owns its replacement. Reopen it.
        my @opened = stat($lock);
        my @named = stat($spec->[0]);
        next unless @named && $opened[0] == $named[0] && $opened[1] == $named[1];
        push @locks, $lock;
        if ($spec->[0] eq "$home/.cache/threadbear-perf.lock") { stop_waiting(); write_holder(); $wrote_holder = 1; }
        last;
    }
}
defer_to_waiters();
$ENV{THREADBEAR_PERF_LOCK_OWNER} = "$host:$$:$home";
my $registry = tempdir('perf-children-XXXXXXXX', DIR => "$home/.cache/threadbear-check");
open(my $registry_state, '>', "$registry/state") or die "perf-lock: registry: $!\n";
close $registry_state;
$ENV{THREADBEAR_PERF_CHILDREN} = $registry;

my @command = @ARGV;
if ($desk) {
    die "perf-lock: --desk needs a fresh pidfile\n" if $pidfile eq '-' || -e $pidfile || -e "$pidfile.gone";
    my $quote = sub { my $s = shift; $s =~ s/'/'\\''/g; return "'$s'"; };
    my $line = join(' ', map { $quote->($_) }
        ('env', "THREADBEAR_PERF_LOCK_OWNER=$ENV{THREADBEAR_PERF_LOCK_OWNER}",
         "THREADBEAR_PERF_CHILDREN=$registry",
         'perl', '-e', 'setpgrp(0, 0); exec @ARGV',
         'perl', "$FindBin::Bin/perf-children.pl", '--child', $registry, '/bin/bash', '-c',
         'echo $$ >"$1"; [ ! -e "$1.gone" ] || exit 129; shift; exec "$@"',
         'perf-desk', $pidfile, @command));
    @command = ("$home/.local/bin/desk", $line);
}
# The guard gets a pipe of its own. Only this supervisor writes it; closing it
# on TERM or ssh EOF asks the guard to clean up while we keep both locks.
pipe(my $read, my $write) or die "perf-lock: pipe: $!\n";
my $child = fork() // die "perf-lock: fork: $!\n";
if (!$child) {
    # A terminal/group cancellation must reach this supervisor, whose pipe
    # then asks the guard to clean up. It mustn't kill the guard and watcher.
    setpgrp(0, 0);
    $SIG{PIPE} = 'DEFAULT';
    close $write;
    # The guard keeps copies too: even KILL of this supervisor must leave the
    # locks held while EOF makes the guard stop its command. Use high fds,
    # since the guard uses fd 3 for its channel. dup2 clears close-on-exec.
    for my $i (0 .. $#locks) {
        dup2(fileno($locks[$i]), 198 + $i) >= 0 or die "perf-lock: dup: $!\n";
    }
    close $_ for @locks;
    open(STDIN, '<&', $read) or die "perf-lock: stdin: $!\n";
    close $read;
    # The guard holds the copies, its workload doesn't. A successful run can
    # leave a harmless heartbeat sleeping briefly; it mustn't keep the turn.
    exec 'perl', "$FindBin::Bin/perf-children.pl", '--reap', $registry, $pidfile,
        '/bin/bash', '-c', 'exec 198>&- 199>&-; exec "$@"', 'perf-command',
        'perl', "$FindBin::Bin/perf-children.pl", '--child', $registry, @command;
    die "perf-lock: guard: $!\n";
}
close $read;
my $closed = 0;
for (;;) {
    my $done = waitpid($child, WNOHANG);
    if ($done == $child) {
        my $status = $?;
        close $write unless $closed;
        exit($cancelled || ($status & 127 ? 128 + ($status & 127) : $status >> 8));
    }
    if (stopping() && !$closed) { close $write; $closed = 1; }
    select(undef, undef, undef, 0.1);
}
