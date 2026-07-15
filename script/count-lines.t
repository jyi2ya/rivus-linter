#!/usr/bin/env perl

use v5.32;
use strict;
use warnings;
use File::Path qw(make_path);
use File::Spec;
use File::Temp qw(tempdir);
use FindBin;
use Test::More;

sub rvs_write_file {
    my ($path, $content) = @_;
    open my $handle, '>:encoding(UTF-8)', $path or die "cannot write '$path': $!";
    print {$handle} $content;
    close $handle or die "cannot close '$path': $!";
}

sub rvs_run_counter {
    my (@arguments) = @_;
    my $script = File::Spec->catfile($FindBin::Bin, 'count-lines.pl');
    open my $pipe, '-|', $^X, $script, @arguments
        or die "cannot run '$script': $!";
    local $/;
    my $output = <$pipe>;
    close $pipe;
    return ($? >> 8, defined $output ? $output : '');
}

my $fixture = tempdir('rivus-count-lines-XXXXXX', TMPDIR => 1, CLEANUP => 1);
make_path(File::Spec->catdir($fixture, 'tests'));
make_path(File::Spec->catdir($fixture, 'test'));
make_path(File::Spec->catdir($fixture, 'fixtures'));

rvs_write_file(
    File::Spec->catfile($fixture, 'main.rs'),
    <<'RUST',
#![allow(dead_code)]
// A line comment is not code.
/* An outer comment.
   /* A nested comment. */
*/
pub fn production<'a>(value: &'a str) -> &'a str {
    let url = "https://example.com//path";
    let raw = r#"/* this is string data */"#;
    let marker = r#"#[cfg(test)]"#;
    if value.is_empty() {
        raw
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_20260715_inline_module_is_ignored() {
        assert_eq!(super::production(""), "/* this is string data */");
    }
}

#[cfg(test)]
pub fn test_only_helper() -> usize {
    99
}

#[test]
fn test_20260715_bare_test_is_ignored() {}

#[tokio::test]
async fn test_20260715_macro_test_is_ignored() {}

#[cfg(test)]
mod hidden_support;

#[cfg(not(test))]
pub fn enabled() {}

#[path = "tests/production_support.rs"]
mod production_support;

#[path = "test/production_helper.rs"]
mod production_helper;

#[path = "fixtures/production_fixture.rs"]
mod production_fixture;
RUST
);

rvs_write_file(
    File::Spec->catfile($fixture, 'library.rs'),
    <<'RUST',
pub struct Item {
    pub value: usize,
}

/* Nested comments are valid: /* inner */ outer. */
pub const VALUE: usize = 1;
RUST
);

rvs_write_file(
    File::Spec->catfile($fixture, 'hidden_support.rs'),
    "pub fn should_not_count() { unreachable!() }\n",
);
rvs_write_file(
    File::Spec->catfile($fixture, 'macro_boundary.rs'),
    <<'RUST',
#[cfg(test)]
test_only_items! {
    struct Ignored;
}
pub fn production_after_macro() {}
RUST
);
rvs_write_file(
    File::Spec->catfile($fixture, 'cfg_forms.rs'),
    <<'RUST',
#[cfg(all(test, unix))]
pub fn all_test_only() {}

#[cfg(any(test, unix))]
pub fn maybe_production() {}
RUST
);
rvs_write_file(
    File::Spec->catfile($fixture, 'attached_attributes.rs'),
    <<'RUST',
#[allow(dead_code)]
#[doc = "test-only helper"]
#[cfg(test)]
pub fn attributed_test_only() {}

pub fn production_after_attributes() {}
RUST
);
rvs_write_file(
    File::Spec->catfile($fixture, 'tests', 'production_support.rs'),
    "pub fn production_support() {\n}\n",
);
rvs_write_file(
    File::Spec->catfile($fixture, 'test', 'production_helper.rs'),
    "pub fn production_helper() {\n}\n",
);
rvs_write_file(
    File::Spec->catfile($fixture, 'fixtures', 'production_fixture.rs'),
    "pub fn production_fixture() {\n}\n",
);
rvs_write_file(
    File::Spec->catfile($fixture, 'tests', 'integration.rs'),
    "fn integration_test_fixture() { unreachable!() }\n",
);

my ($status, $output) = rvs_run_counter($fixture);
is($status, 0, 'test_20260715_count_lines_exit_status');

my $snapshot_path = File::Spec->catfile(
    $FindBin::Bin,
    '..',
    'test_out',
    'test_20260715_count_lines.out',
);
open my $snapshot_handle, '<:encoding(UTF-8)', $snapshot_path
    or die "cannot read '$snapshot_path': $!";
local $/;
my $expected = <$snapshot_handle>;
close $snapshot_handle or die "cannot close '$snapshot_path': $!";
is($output, $expected, 'test_20260715_count_lines_snapshot');

my ($by_file_status, $by_file_output) = rvs_run_counter('--by-file', $fixture);
is($by_file_status, 0, 'test_20260715_count_lines_by_file_exit_status');
like($by_file_output, qr/^\s*19\s+main\.rs$/m, 'test_20260715_count_lines_main_breakdown');
like($by_file_output, qr/^\s*4\s+library\.rs$/m, 'test_20260715_count_lines_library_breakdown');
like(
    $by_file_output,
    qr/^\s*1\s+macro_boundary\.rs$/m,
    'test_20260715_test_only_braced_item_macro_preserves_following_item',
);
like(
    $by_file_output,
    qr/^\s*2\s+cfg_forms\.rs$/m,
    'test_20260715_cfg_all_test_is_excluded_and_cfg_any_test_is_conservative',
);
like(
    $by_file_output,
    qr/^\s*1\s+attached_attributes\.rs$/m,
    'test_20260715_test_only_item_removes_preceding_attached_attributes',
);
like(
    $by_file_output,
    qr/^\s*2\s+tests\/production_support\.rs$/m,
    'test_20260715_path_referenced_tests_directory_is_counted',
);
like(
    $by_file_output,
    qr/^\s*2\s+test\/production_helper\.rs$/m,
    'test_20260715_path_referenced_test_directory_is_counted',
);
like(
    $by_file_output,
    qr/^\s*2\s+fixtures\/production_fixture\.rs$/m,
    'test_20260715_path_referenced_fixtures_directory_is_counted',
);
unlike($by_file_output, qr/hidden_support|integration/, 'test_20260715_count_lines_excludes_test_files');

done_testing();
