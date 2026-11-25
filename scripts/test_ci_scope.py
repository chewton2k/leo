import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
FLAGS = ('rust', 'browser', 'node', 'installers')


class ScopeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = Path(self.tmp.name)
        self.env = dict(os.environ, GIT_AUTHOR_NAME='ci-test', GIT_AUTHOR_EMAIL='ci@example.com',
                        GIT_COMMITTER_NAME='ci-test', GIT_COMMITTER_EMAIL='ci@example.com')
        self.git('init', '-q')
        self.write('README.md')
        self.commit()
        self.base = self.git('rev-parse', 'HEAD').strip()

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.repo, env=self.env,
                                       stderr=subprocess.STDOUT, text=True, timeout=10)

    def write(self, name):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('changed\n')

    def commit(self):
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture')

    def scope(self, *paths, base=None):
        for path in paths:
            self.write(path)
        if paths:
            self.commit()
        output = subprocess.check_output(['sh', str(ROOT / 'scripts/ci-scope.sh'),
                                          self.base if base is None else base],
                                         cwd=self.repo, text=True, timeout=10)
        return dict(line.split('=', 1) for line in output.splitlines())

    def assert_scope(self, result, *enabled):
        self.assertEqual(set(result), {*FLAGS, 'systems'})
        for flag in FLAGS:
            self.assertEqual(result[flag], str(flag in enabled).lower(), flag)
        expected = ['macos-latest', 'windows-latest'] if 'rust' in enabled else []
        self.assertEqual(json.loads(result['systems']), expected)

    def test_docs_and_empty_diffs_need_no_expensive_jobs(self):
        self.assert_scope(self.scope())
        self.assert_scope(self.scope('guide.md', 'docs/design.txt', 'LICENSE'))

    def test_page_changes_run_node_and_both_browser_suites(self):
        self.assert_scope(self.scope('crates/leo-web/src/web/vendor/mermaid.min.js'), 'node', 'browser')

    def test_page_scripts_scanned_by_rust_tests_also_run_rust(self):
        for path in ('crates/leo-web/src/web/app/notes.js', 'crates/leo-web/src/web/chat.js'):
            with self.subTest(path=path):
                self.assert_scope(self.scope(path), 'rust', 'node', 'browser')
                self.base = self.git('rev-parse', 'HEAD').strip()

    def test_readme_and_contributing_are_read_by_the_wording_test(self):
        for path in ('README.md', 'CONTRIBUTING.md'):
            with self.subTest(path=path):
                (self.repo / path).write_text('edited\n')
                self.commit()
                self.assert_scope(self.scope(), 'rust')
                self.base = self.git('rev-parse', 'HEAD').strip()

    def test_browser_test_server_runs_the_node_test_that_guards_it(self):
        self.assert_scope(self.scope('crates/leo-web/tests/browser/server.js'), 'node', 'browser')

    def test_page_parts_compiled_into_rust_also_run_rust(self):
        for path in ('crates/leo-web/src/web/styles/09-kanban.css', 'crates/leo-web/src/web/page-head.html', 'crates/leo-web/src/web/page-tail.html'):
            self.assert_scope(self.scope(path), 'rust', 'node', 'browser')

    def test_browser_specs_do_not_require_rust_unit_tests(self):
        self.assert_scope(self.scope('crates/leo-web/tests/browser/flows.spec.js'), 'browser')

    def test_node_specs_do_not_require_rust_or_browsers(self):
        self.assert_scope(self.scope('crates/leo-web/tests/saving.test.js'), 'node')

    def test_node_support_files_run_node(self):
        self.assert_scope(self.scope('crates/leo-web/tests/chat.test.js'), 'node')

    def test_page_source_shared_with_browser_specs_runs_both(self):
        self.assert_scope(self.scope('crates/leo-web/tests/page-source.js'), 'browser', 'node')

    def test_rust_sources_read_by_node_tests_run_node(self):
        for path in ('crates/leo-web/src/routes/assets.rs', 'crates/leo-web/src/routes/mod.rs'):
            with self.subTest(path=path):
                self.assert_scope(self.scope(path), 'rust', 'browser', 'node')
                self.base = self.git('rev-parse', 'HEAD').strip()
        for path in ('crates/leo-desktop/src/lib.rs', 'crates/leo-desktop/permissions/print.json',
                     'crates/leo-desktop/capabilities/main.json', 'crates/leo-desktop/dist/dictation.js'):
            with self.subTest(path=path):
                self.assert_scope(self.scope(path), 'rust', 'node')
                self.base = self.git('rev-parse', 'HEAD').strip()

    def test_shared_rust_changes_keep_cross_platform_and_browser_coverage(self):
        self.assert_scope(self.scope('crates/leo-core/src/store.rs'), 'rust', 'browser')

    def test_desktop_and_cli_changes_keep_cross_platform_coverage(self):
        self.assert_scope(self.scope('crates/leo-desktop/src/notify.rs', 'src/main.rs'), 'rust')

    def test_rust_integration_tests_run_without_browsers(self):
        self.assert_scope(self.scope('tests/e2e.rs', 'crates/leo-web/tests/markdown.rs'), 'rust')

    def test_installer_changes_run_their_own_checks(self):
        self.assert_scope(self.scope('scripts/install-app.ps1', 'scripts/test_desktop_release.py'), 'rust', 'installers')

    def test_unknown_inputs_and_workflow_changes_fail_open_to_full_coverage(self):
        for path in ('Cargo.lock', '.cargo/config.toml', '.github/workflows/ci.yml',
                     'scripts/ci-scope.sh', 'new-build-input.txt'):
            with self.subTest(path=path):
                self.assert_scope(self.scope(path), *FLAGS)
                self.base = self.git('rev-parse', 'HEAD').strip()

    def test_missing_and_invalid_bases_run_everything(self):
        self.assert_scope(self.scope(base=''), *FLAGS)
        self.assert_scope(self.scope(base='does-not-exist'), *FLAGS)

    def test_mixed_changes_union_the_checks(self):
        self.assert_scope(self.scope('scripts/install-app.sh', 'crates/leo-web/tests/saving.test.js'),
                          'rust', 'installers', 'node')

    def test_failed_or_cancelled_intermediate_changes_remain_in_scope(self):
        self.write('crates/leo-core/src/store.rs')
        self.commit()
        self.assert_scope(self.scope('new-guide.md'), 'rust', 'browser')

    def test_deletions_and_renames_keep_both_sides_in_scope(self):
        self.write('crates/leo-core/src/old.rs')
        self.commit()
        self.base = self.git('rev-parse', 'HEAD').strip()
        self.git('mv', 'crates/leo-core/src/old.rs', 'guide.md')
        self.commit()
        self.assert_scope(self.scope(), 'rust', 'browser')

    def test_deleted_sources_still_select_their_checks(self):
        self.write('crates/leo-core/src/old.rs')
        self.commit()
        self.base = self.git('rev-parse', 'HEAD').strip()
        self.git('rm', 'crates/leo-core/src/old.rs')
        self.commit()
        self.assert_scope(self.scope(), 'rust', 'browser')

    def test_mixed_scopes_are_independent_of_path_order(self):
        for paths in [('scripts/install-app.sh', 'crates/leo-web/src/web/doc.js'),
                      ('crates/leo-web/src/web/core.js', 'scripts/install-app.ps1')]:
            self.assert_scope(self.scope(*paths), 'rust', 'browser', 'node', 'installers')
            self.base = self.git('rev-parse', 'HEAD').strip()


if __name__ == '__main__':
    unittest.main()
