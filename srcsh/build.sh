#!/usr/bin/env bash

usage() {
	cat <<-EOF
	$0 [-h] [ -m /path/to/md_to_tex ] [ -x[x[x]]]

	Execute the md_to_tex executable on the files ../content.md and ../abstract.md,
	outputting the results in this directory and optionally executing
	xelatex on the paper.tex file.

	Options:
	-h          Show this message and exit
	-m <path>   Use the supplied path to the md_to_tex file.
	            Default behavior searches PATH.
	-x          Execute xelatex on the file.  May be supplied up to three times.
	            It is common to run xelatex twice since the first time discovers
	            references, figures, etc and the second time the links are generated.
	            If supplied 3 times, bibtex is run after the first xelatex.

	EOF

	if [ $# -gt 0 ]; then
		echo "$*" >&2
		exit 1
	fi
	exit 0
}

MD_TO_TEX=
TEX_COUNT=0
while [ $# -gt 0 ]; do
	case "$1" in
		h | -h | help | -help | --help )
			usage
			;;
		m | -m | --md-exec )
			MD_TO_TEX="$2"
			[ -x "$MD_TO_TEX" ] || usage "The supplied path to md_to_tex does not exist or is not executable"
			shift
			;;
		xxx | -xxx )
			(( TEX_COUNT += 3 ))
			;;
		xx | -xx )
			(( TEX_COUNT += 2 ))
			;;
		x | -x ) 
			(( TEX_COUNT += 1 ))
			;;
	esac
	shift
done

if [ -z "$MD_TO_TEX" ]; then
	MD_TO_TEX="$(which md_to_tex)" || usage "No md_to_tex in the PATH"
fi

if [ "$TEX_COUNT" -gt 4 ]; then
	usage "The '-x' argument was supplied too many times"
fi


$MD_TO_TEX -f "../content.md" > "content.tex"
$MD_TO_TEX -f "../abstract.md" > "abstract.tex"

if [ $TEX_COUNT -eq 3 ]; then
	xelatex paper.tex && \
		bibtex paper && \
		xelatex paper.tex && \
		xelatex paper.tex
elif [ $TEX_COUNT -eq 2 ]; then
	xelatex paper.tex && \
		xelatex paper.tex
elif [ $TEX_COUNT -eq 1 ]; then
	xelatex paper.tex
fi

