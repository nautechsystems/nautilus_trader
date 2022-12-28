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

# ContingencyType <1385> field
# FIX 5.0 SP2 EP266
# https://www.onixs.biz/fix-dictionary/5.0.sp2.ep266/tagNum_1385.html
# https://www.onixs.biz/fix-dictionary/5.0.sp2/glossary.html#OneCancelsTheOther


cpdef enum ContingencyType:
    NONE = 0
    OCO = 1  # One Cancels Other
    OTO = 2  # One Triggers Other
    OUO = 3  # One Updates Other
