# fmt: off
from pygments.style import Style
from pygments.token import Comment
from pygments.token import Error
from pygments.token import Generic
from pygments.token import Keyword
from pygments.token import Literal
from pygments.token import Name
from pygments.token import Number
from pygments.token import Operator
from pygments.token import Other
from pygments.token import Punctuation
from pygments.token import String
from pygments.token import Text
from pygments.token import Whitespace


# flake8: noqa


class MonokaiStyle(Style):
    """
    This style mimics the Monokai color scheme.
    """

    background_color = "rgb(0 0 0 / 12%)"
    highlight_color = "#49483e"

    styles = {
        # No corresponding class for the following:
        Text:                    "#f8f8f2",              # class:  ''
        Whitespace:              "",                     # class: 'w'
        Error:                   "#960050 bg:#1e0010",   # class: 'err'
        Other:                   "",                     # class 'x'

        Comment:                 "#888",                 # class: 'c'
        Comment.Multiline:       "",                     # class: 'cm'
        Comment.Preproc:         "",                     # class: 'cp'
        Comment.Single:          "",                     # class: 'c1'
        Comment.Special:         "",                     # class: 'cs'

        Keyword:                 "#D9C4FF",              # class: 'k'
        Keyword.Constant:        "",                     # class: 'kc'
        Keyword.Declaration:     "",                     # class: 'kd'
        Keyword.Namespace:       "#D9C4FF",              # class: 'kn'
        Keyword.Pseudo:          "",                     # class: 'kp'
        Keyword.Reserved:        "",                     # class: 'kr'
        Keyword.Type:            "",                     # class: 'kt'

        Operator:                "#f92672",              # class: 'o'
        Operator.Word:           "",                     # class: 'ow' - like keywords

        Punctuation:             "#f8f8f2",              # class: 'p'

        Name:                    "#f8f8f2",              # class: 'n'
        Name.Attribute:          "#D9C4FF",              # class: 'na' - to be revised
        Name.Builtin:            "",                     # class: 'nb'
        Name.Builtin.Pseudo:     "",                     # class: 'bp'
        Name.Class:              "#A0D8F0",              # class: 'nc' - to be revised
        Name.Constant:           "#A0D8F0",              # class: 'no' - to be revised
        Name.Decorator:          "#e6db74",              # class: 'nd' - to be revised
        Name.Entity:             "",                     # class: 'ni'
        Name.Exception:          "#D9C4FF",              # class: 'ne'
        Name.Function:           "#A0D8F0",              # class: 'nf'
        Name.Property:           "",                     # class: 'py'
        Name.Label:              "",                     # class: 'nl'
        Name.Namespace:          "",                     # class: 'nn' - to be revised
        Name.Other:              "#D9C4FF",              # class: 'nx'
        Name.Tag:                "#f92672",              # class: 'nt' - like a keyword
        Name.Variable:           "",                     # class: 'nv' - to be revised
        Name.Variable.Class:     "#A0D8F0",              # class: 'vc' - to be revised
        Name.Variable.Global:    "",                     # class: 'vg' - to be revised
        Name.Variable.Instance:  "",                     # class: 'vi' - to be revised

        Number:                  "#e6db74",              # class: 'm'
        Number.Float:            "",                     # class: 'mf'
        Number.Hex:              "",                     # class: 'mh'
        Number.Integer:          "",                     # class: 'mi'
        Number.Integer.Long:     "",                     # class: 'il'
        Number.Oct:              "",                     # class: 'mo'

        Literal:                 "#D9C4FF",              # class: 'l'
        Literal.Date:            "#A3BE8C",              # class: 'ld'

        String:                  "#A3BE8C",              # class: 's'
        String.Backtick:         "",                     # class: 'sb'
        String.Char:             "",                     # class: 'sc'
        String.Doc:              "",                     # class: 'sd' - like a comment
        String.Double:           "",                     # class: 's2'
        String.Escape:           "#D9C4FF",              # class: 'se'
        String.Heredoc:          "",                     # class: 'sh'
        String.Interpol:         "",                     # class: 'si'
        String.Other:            "",                     # class: 'sx'
        String.Regex:            "",                     # class: 'sr'
        String.Single:           "",                     # class: 's1'
        String.Symbol:           "",                     # class: 'ss'

        Generic:                 "",                     # class: 'g'
        Generic.Deleted:         "#f92672",              # class: 'gd',
        Generic.Emph:            "italic",               # class: 'ge'
        Generic.Error:           "",                     # class: 'gr'
        Generic.Heading:         "",                     # class: 'gh'
        Generic.Inserted:        "#D9C4FF",              # class: 'gi'
        Generic.Output:          "",                     # class: 'go'
        Generic.Prompt:          "",                     # class: 'gp'
        Generic.Strong:          "bold",                 # class: 'gs'
        Generic.Subheading:      "#75715e",              # class: 'gu'
        Generic.Traceback:       "",                     # class: 'gt'
    }
