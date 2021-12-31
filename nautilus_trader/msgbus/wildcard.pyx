# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------


cpdef bint is_matching(str topic, str pattern) except *:
    """
    Return a value indicating whether the topic matches with the pattern.

    Given a topic and pattern potentially containing wildcard characters, i.e.
    `*` and `?`, where `?` can match any single character in the topic, and `*`
    can match any number of characters including zero characters.

    Parameters
    ----------
    topic : str
        The topic string.
    pattern : str
        The pattern to match on.

    Returns
    -------
    bool

    """
    # Get length of string and wildcard pattern
    cdef int n = len(topic)
    cdef int m = len(pattern)

    # Create a DP lookup table
    cdef list t = [[False for x in range(m + 1)] for y in range(n + 1)]

    # If both pattern and string are empty: match
    t[0][0] = True

    # Handle empty string case (i == 0)
    cdef int j
    for j in range(1, m + 1):
        if pattern[j - 1] == '*':
            t[0][j] = t[0][j - 1]

    # Build a matrix in a bottom-up manner
    cdef int i
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            if pattern[j - 1] == '*':
                t[i][j] = t[i - 1][j] or t[i][j - 1]
            elif pattern[j - 1] == '?' or topic[i - 1] == pattern[j - 1]:
                t[i][j] = t[i - 1][j - 1]

    # Last cell stores the answer
    return t[n][m]
