#!/usr/bin/env perl

use v5.32;
use strict;
use warnings;
use Cwd qw(abs_path);
use File::Basename qw(basename dirname);
use File::Find qw(find);
use File::Spec;

my $CFG_TEST_RE = qr{
    \#\s*\[\s*cfg\s*\(\s*
    (?:
        test
        |
        all\s*\(\s*test(?:\s*,\s*[^(),]+)*\s*\)
    )
    \s*\)\s*\]
}x;
my $TEST_ATTR_RE = qr/\#\s*\[\s*(?:(?:[A-Za-z_]\w*)\s*::\s*)*test(?:\s*\([^\]\n]*\))?\s*\]/;

sub rvs_usage {
    my ($exit_code) = @_;
    my $stream = $exit_code == 0 ? *STDOUT : *STDERR;
    print {$stream} "Usage: script/count-lines.pl [--by-file] DIRECTORY\n";
    exit $exit_code;
}

sub rvs_read_source {
    my ($path) = @_;
    open my $handle, '<:encoding(UTF-8)', $path
        or die "cannot read '$path': $!\n";
    local $/;
    my $source = <$handle>;
    close $handle or die "cannot close '$path': $!\n";
    return defined $source ? $source : '';
}

sub rvs_blank_chars {
    my ($chars, $start, $length) = @_;
    my $end = $start + $length;
    for (my $i = $start; $i < $end; $i++) {
        $chars->[$i] = ' ' if $chars->[$i] ne "\n";
    }
}

# Replace comments and literal contents with spaces while preserving offsets and
# newlines. Literal delimiters remain so a literal expression still counts.
sub rvs_mask_non_code {
    my ($source) = @_;
    my @chars = split //, $source;
    my $length = scalar @chars;
    my $i = 0;

    while ($i < $length) {
        my $tail = substr($source, $i);

        if ($tail =~ m{\A//}) {
            my $end = index($source, "\n", $i + 2);
            $end = $length if $end < 0;
            rvs_blank_chars(\@chars, $i, $end - $i);
            $i = $end;
            next;
        }

        if ($tail =~ m{\A/\*}) {
            my $start = $i;
            my $depth = 1;
            $i += 2;
            while ($i < $length && $depth > 0) {
                if (substr($source, $i, 2) eq '/*') {
                    $depth++;
                    $i += 2;
                } elsif (substr($source, $i, 2) eq '*/') {
                    $depth--;
                    $i += 2;
                } else {
                    $i++;
                }
            }
            die "unclosed block comment\n" if $depth != 0;
            rvs_blank_chars(\@chars, $start, $i - $start);
            next;
        }

        if ($tail =~ /\A((?:br|rb|cr|rc|r)(\#{0,255})")/) {
            my $opening = $1;
            my $hashes = $2;
            $i += length $opening;
            my $content_start = $i;
            my $closing = '"' . $hashes;
            my $end = index($source, $closing, $i);
            die "unclosed raw string literal\n" if $end < 0;
            rvs_blank_chars(\@chars, $content_start, $end - $content_start);
            $i = $end + length $closing;
            next;
        }

        if ($tail =~ /\A((?:b|c)?")/) {
            my $opening = $1;
            $i += length $opening;
            while ($i < $length) {
                if ($chars[$i] eq '\\') {
                    rvs_blank_chars(\@chars, $i, 1);
                    $i++;
                    if ($i < $length) {
                        rvs_blank_chars(\@chars, $i, 1);
                        $i++;
                    }
                } elsif ($chars[$i] eq '"') {
                    $i++;
                    last;
                } else {
                    rvs_blank_chars(\@chars, $i, 1);
                    $i++;
                }
            }
            die "unclosed string literal\n"
                if $i >= $length && ($length == 0 || $chars[$length - 1] ne '"');
            next;
        }

        if ($tail =~ /\A(?:b)?'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\}|[^\n])|[^\\'\n])'/) {
            $i += length $&;
            next;
        }

        $i++;
    }

    return join '', @chars;
}

sub rvs_matching_brace_end {
    my ($masked, $start) = @_;
    my $depth = 0;
    my $length = length $masked;
    for (my $i = $start; $i < $length; $i++) {
        my $char = substr($masked, $i, 1);
        if ($char eq '{') {
            $depth++;
        } elsif ($char eq '}') {
            $depth--;
            return $i + 1 if $depth == 0;
        }
    }
    die "unclosed item body\n";
}

sub rvs_header_is_braced_item_macro {
    my ($header) = @_;
    my $candidate = $header;
    while ($candidate =~ s/\A\s*\#\s*\[[^\[\]]*\]\s*//s) {
        # Strip attributes between the test marker and the macro invocation.
    }
    $candidate =~ s/\A\s*pub(?:\s*\([^)]*\))?\s+//;
    return $candidate =~ /\A\s*(?:(?:[A-Za-z_]\w*)\s*::\s*)*[A-Za-z_]\w*\s*!\s*\z/s;
}

sub rvs_item_end_after_attribute {
    my ($masked, $start) = @_;
    my $length = length $masked;
    my $paren_depth = 0;
    my $bracket_depth = 0;
    my $header = '';
    my $i = $start;

    while ($i < $length) {
        my $char = substr($masked, $i, 1);
        if ($char eq '(') {
            $paren_depth++;
        } elsif ($char eq ')') {
            $paren_depth--;
        } elsif ($char eq '[') {
            $bracket_depth++;
        } elsif ($char eq ']') {
            $bracket_depth--;
        } elsif ($paren_depth == 0 && $bracket_depth == 0 && $char eq '{') {
            my $end = rvs_matching_brace_end($masked, $i);
            my $has_item_body = $header =~ /\b(?:fn|mod|impl|trait|struct|enum|union|extern)\b/
                || $header =~ /\bmacro_rules\s*!/
                || rvs_header_is_braced_item_macro($header);
            if ($has_item_body) {
                $end++ while $end < $length && substr($masked, $end, 1) =~ /\s/;
                $end++ if $end < $length && substr($masked, $end, 1) =~ /[;,]/;
                return $end;
            }
            $i = $end;
            next;
        } elsif ($paren_depth == 0 && $bracket_depth == 0 && $char =~ /[;,]/) {
            return $i + 1;
        }

        $header .= $char if $paren_depth == 0 && $bracket_depth == 0;
        $i++;
    }

    die "cannot find the end of test-only item\n";
}

sub rvs_attached_attribute_start {
    my ($masked, $start) = @_;
    my $cursor = $start;

    while (1) {
        my $probe = $cursor;
        $probe-- while $probe > 0 && substr($masked, $probe - 1, 1) =~ /\s/;
        last if $probe == 0 || substr($masked, $probe - 1, 1) ne ']';

        my $depth = 0;
        my $open = -1;
        for (my $i = $probe - 1; $i >= 0; $i--) {
            my $char = substr($masked, $i, 1);
            if ($char eq ']') {
                $depth++;
            } elsif ($char eq '[') {
                $depth--;
                if ($depth == 0) {
                    $open = $i;
                    last;
                }
            }
        }
        last if $open < 0;

        my $hash = $open;
        $hash-- while $hash > 0 && substr($masked, $hash - 1, 1) =~ /\s/;
        last if $hash == 0 || substr($masked, $hash - 1, 1) ne '#';
        $cursor = $hash - 1;
    }

    return $cursor;
}

sub rvs_test_ranges {
    my ($masked) = @_;
    my @ranges;
    pos($masked) = 0;

    while ($masked =~ /(?:$CFG_TEST_RE|$TEST_ATTR_RE)/g) {
        my $start = rvs_attached_attribute_start($masked, $-[0]);
        my $end = rvs_item_end_after_attribute($masked, $+[0]);
        push @ranges, [$start, $end];
        pos($masked) = $end;
    }

    return @ranges;
}

sub rvs_external_test_modules {
    my ($path, $masked) = @_;
    my @paths;
    while ($masked =~ /$CFG_TEST_RE\s*(?:\#\s*\[[^\]]*\]\s*)*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+([A-Za-z_]\w*)\s*;/g) {
        my $name = $1;
        push @paths, File::Spec->canonpath(File::Spec->catfile(dirname($path), "$name.rs"));
        push @paths,
            File::Spec->canonpath(File::Spec->catfile(dirname($path), $name, 'mod.rs'));
    }
    return @paths;
}

sub rvs_offset_is_in_ranges {
    my ($offset, $ranges) = @_;
    for my $range (@{$ranges}) {
        return 1 if $offset >= $range->[0] && $offset < $range->[1];
    }
    return 0;
}

sub rvs_path_attribute_value {
    my ($attribute) = @_;
    if ($attribute =~ /\bpath\s*=\s*r(\#{0,255})"(.*?)"\1/s) {
        return $2;
    }
    return if $attribute !~ /\bpath\s*=\s*"((?:\\.|[^"\\])*)"/s;
    my $value = $1;
    return if $value =~ /\\(?!["\\])/;
    $value =~ s/\\(["\\])/$1/g;
    return $value;
}

sub rvs_explicit_production_module_paths {
    my ($path, $source, $masked, $test_ranges) = @_;
    my @paths;
    pos($masked) = 0;

    while ($masked =~ /\#\s*\[\s*path\s*=/g) {
        my $start = $-[0];
        next if rvs_offset_is_in_ranges($start, $test_ranges);

        my $end = index($masked, ']', $+[0]);
        next if $end < 0;
        $end++;
        my $after = substr($masked, $end);
        next if $after !~ /\A\s*(?:\#\s*\[[^\]]*\]\s*)*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+[A-Za-z_]\w*\s*;/s;

        my $attribute = substr($source, $start, $end - $start);
        my $relative = rvs_path_attribute_value($attribute);
        next if !defined $relative || $relative eq '';
        push @paths, File::Spec->catfile(dirname($path), $relative);
    }

    return @paths;
}

sub rvs_path_is_within_root {
    my ($path, $root) = @_;
    my $relative = File::Spec->abs2rel($path, $root);
    return $relative !~ /\A\.\.(?:[\\\/]|\z)/;
}

sub rvs_production_lines {
    my ($masked) = @_;
    my @chars = split //, $masked;
    for my $range (rvs_test_ranges($masked)) {
        rvs_blank_chars(\@chars, $range->[0], $range->[1] - $range->[0]);
    }

    my $filtered = join '', @chars;
    my $count = 0;
    $count++ for grep { /\S/ } split /\n/, $filtered, -1;
    return $count;
}

my $by_file = 0;
if (@ARGV && $ARGV[0] eq '--by-file') {
    $by_file = 1;
    shift @ARGV;
}
rvs_usage(0) if @ARGV == 1 && $ARGV[0] eq '--help';
rvs_usage(2) if @ARGV != 1;

my $root = abs_path($ARGV[0]);
die "'$ARGV[0]' is not a directory\n" if !defined $root || !-d $root;

my %excluded_directories = map { $_ => 1 } qw(.git target test tests test_out fixtures benches);
my @paths;
find(
    {
        no_chdir => 1,
        wanted => sub {
            my $path = $File::Find::name;
            if (-d $path) {
                if ($path ne $root && $excluded_directories{basename($path)}) {
                    $File::Find::prune = 1;
                }
                return;
            }
            push @paths, File::Spec->canonpath($path) if -f $path && $path =~ /\.rs\z/;
        },
    },
    $root,
);

my %masked_by_path;
my %selected_path = map { $_ => 1 } @paths;
my %explicit_production_path;
for (my $i = 0; $i < @paths; $i++) {
    my $path = $paths[$i];
    my $source = rvs_read_source($path);
    my $masked = rvs_mask_non_code($source);
    $masked_by_path{$path} = $masked;
    my @test_ranges = rvs_test_ranges($masked);
    for my $referenced (
        rvs_explicit_production_module_paths($path, $source, $masked, \@test_ranges)
    ) {
        my $resolved = abs_path($referenced);
        next if !defined $resolved || !-f $resolved || !rvs_path_is_within_root($resolved, $root);
        $resolved = File::Spec->canonpath($resolved);
        $explicit_production_path{$resolved} = 1;
        next if $selected_path{$resolved};
        $selected_path{$resolved} = 1;
        push @paths, $resolved;
    }
}

my %external_test_path;
for my $path (@paths) {
    $external_test_path{$_} = 1
        for rvs_external_test_modules($path, $masked_by_path{$path});
}

my @results;
my $total = 0;
for my $path (sort @paths) {
    next if $external_test_path{$path} && !$explicit_production_path{$path};
    my $lines = rvs_production_lines($masked_by_path{$path});
    my $relative = File::Spec->abs2rel($path, $root);
    $relative =~ s{\\}{/}g;
    push @results, [$relative, $lines];
    $total += $lines;
}

if ($by_file) {
    printf "%7d  %s\n", $_->[1], $_->[0] for @results;
    print "--------\n";
}
printf "Rust files: %d\n", scalar @results;
printf "Production code lines: %d\n", $total;
