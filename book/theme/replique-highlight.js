// Syntax highlighting of the `replique` code blocks.
//
// mdBook highlights with highlight.js, which does not know the format. This
// registers it, then highlights the blocks again: by the time this script
// runs, mdBook has already been through them without finding the language.
(function () {
    'use strict';

    if (typeof hljs === 'undefined') {
        return;
    }

    hljs.registerLanguage('replique', function (hljs) {
        const IDENT = /[A-Za-z_][A-Za-z0-9_]*/;
        const EXPR_KEYWORDS = {
            keyword: 'and or not has',
            literal: 'true false',
        };

        const COMMENT = hljs.COMMENT(/\/\//, /$/);

        // What an expression is made of. The operators are left as they are.
        const EXPRESSION = [
            { className: 'variable', begin: /\$[A-Za-z_][A-Za-z0-9_]*/ },
            { className: 'attr', begin: /\.[A-Za-z_][A-Za-z0-9_]*/ },
            { className: 'string', begin: /"/, end: /"|$/, contains: [hljs.BACKSLASH_ESCAPE] },
            { className: 'number', begin: /\b[0-9]+(?:\.[0-9]*)?/ },
            // `name(`: the parenthesis has to touch the name.
            { className: 'title', begin: /[A-Za-z_][A-Za-z0-9_]*(?=\()/ },
            // `key:` of a dict.
            { className: 'attr', begin: /[A-Za-z_][A-Za-z0-9_]*(?=[ \t]*:)/ },
        ];

        // `:= name`, `=> name` and `<=> name`
        const NODE = {
            begin: /^[ \t]*(?::=|<=>|=>)(?=\s|$)/,
            end: /$/,
            returnBegin: true,
            contains: [
                { className: 'keyword', begin: /:=|<=>|=>/ },
                { className: 'section', begin: IDENT },
                COMMENT,
            ],
        };

        // `---`
        const NODE_END = { className: 'keyword', begin: /^---(?=\s|$)/ };

        // `>> command(args)` and `>> await command(args)`
        const COMMAND = {
            begin: /^[ \t]*>>(?=\s|$)/,
            end: /$/,
            returnBegin: true,
            keywords: {
                keyword: 'await ' + EXPR_KEYWORDS.keyword,
                literal: EXPR_KEYWORDS.literal,
            },
            contains: [{ className: 'keyword', begin: />>/ }, COMMENT].concat(EXPRESSION),
        };

        // `->`: what follows is text, read by the modes below.
        const CHOICE = { className: 'keyword', begin: /^[ \t]*->(?=\s|$)/ };

        // `[let ...]`, `[if ...]`, `[while ...]` and the like, alone on a line.
        const STATEMENT = {
            begin: /^[ \t]*\[(?=(?:let|if|elif|else|while|break|continue)(?![A-Za-z0-9_]))/,
            end: /\]|$/,
            keywords: {
                keyword: 'let if elif else while break continue ' + EXPR_KEYWORDS.keyword,
                literal: EXPR_KEYWORDS.literal,
            },
            contains: [COMMENT].concat(EXPRESSION),
        };

        // `[$name]` in the middle of a text, and the `[if ...]` that ends a
        // choice.
        const INLINE = {
            begin: /\[/,
            end: /\]|$/,
            keywords: {
                keyword: 'if ' + EXPR_KEYWORDS.keyword,
                literal: EXPR_KEYWORDS.literal,
            },
            contains: [COMMENT].concat(EXPRESSION),
        };

        // `Alice:`. Tried last, so that a marker always wins over it.
        const SPEAKER = {
            className: 'name',
            begin: /^[ \t]*[^\s:\[#\/\-=>](?:[^:\n\[#\/]|\/(?!\/))*:/,
        };

        // `#name` and `#name:value`, after a blank or at the start of a text.
        const TAG = {
            className: 'built_in',
            begin: /(?:^|[ \t])#[A-Za-z_][A-Za-z0-9_]*(?::[A-Za-z0-9_.\-]+)?(?=\s|$)/,
        };

        return {
            name: 'Replique',
            aliases: ['rep'],
            contains: [COMMENT, NODE, NODE_END, COMMAND, CHOICE, STATEMENT, SPEAKER, TAG, INLINE],
        };
    });

    document.querySelectorAll('code.language-replique, code.language-rep').forEach(function (block) {
        hljs.highlightBlock(block);
    });
})();
