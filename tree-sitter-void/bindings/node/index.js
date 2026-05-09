try {
  module.exports = require('../../build/Release/tree_sitter_void_binding');
} catch (_) {
  module.exports = require('../../build/Debug/tree_sitter_void_binding');
}
