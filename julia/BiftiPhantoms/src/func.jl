# `func` expressions of transformed references (../../JSON.md, "Transformed reference").
#
# The grammar is parsed exactly - never `eval`ed - so a phantom file cannot run code:
#
#     expr   = term   (("+" | "-") term)*
#     term   = factor (("*" | "/") factor)*
#     factor = number | variable | "(" expr ")" | ("+" | "-") factor
#
# Variables are `x` (the voxel value) and the volume statistics `x_min`, `x_max`,
# `x_mean` and `x_std`.

const FUNC_VARIABLES = (:x, :x_min, :x_max, :x_mean, :x_std)
const FUNC_OPERATORS = Dict('+' => +, '-' => -, '*' => *, '/' => /)
const FUNC_TOKEN = r"\s*(?:(?<number>(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)|(?<name>[A-Za-z_]\w*)|(?<symbol>[-+*/()]))"

mutable struct FuncParser
    const func::String
    const tokens::Vector{Any}
    position::Int
end

function tokenize(func::AbstractString)
    tokens = Any[]
    offset = 1
    while offset <= ncodeunits(func)
        isempty(strip(SubString(func, offset))) && break
        m = match(FUNC_TOKEN, func, offset)
        (isnothing(m) || m.offset != offset) && throw(func_error(func, "unexpected character at position $offset"))
        if !isnothing(m[:number])
            push!(tokens, parse(Float64, m[:number]))
        elseif !isnothing(m[:name])
            name = Symbol(m[:name])
            name in FUNC_VARIABLES || throw(func_error(func, "unknown variable $(m[:name])"))
            push!(tokens, name)
        else
            push!(tokens, only(m[:symbol]))
        end
        offset += ncodeunits(m.match)
    end
    return tokens
end

func_error(func, msg) = ArgumentError("Invalid func $(repr(func)): $msg")

current(p::FuncParser) = get(p.tokens, p.position, nothing)
next!(p::FuncParser) = (token = current(p); p.position += 1; token)

function expect!(p::FuncParser, token)
    next!(p) == token || throw(func_error(p.func, "expected $(repr(token))"))
    return nothing
end

function parse_binary(p::FuncParser, operand, ops)
    lhs = operand(p)
    while current(p) in ops
        op = FUNC_OPERATORS[next!(p)]
        lhs = (op, lhs, operand(p))
    end
    return lhs
end

parse_expr(p::FuncParser) = parse_binary(p, parse_term, ('+', '-'))
parse_term(p::FuncParser) = parse_binary(p, parse_factor, ('*', '/'))

function parse_factor(p::FuncParser)
    token = next!(p)
    token isa Union{Float64,Symbol} && return token
    token == '(' && return (expr = parse_expr(p); expect!(p, ')'); expr)
    token in ('+', '-') && return (FUNC_OPERATORS[token], 0.0, parse_factor(p))
    throw(func_error(p.func, isnothing(token) ? "unexpected end" : "unexpected $(repr(token))"))
end

"""
    parse_func(func) -> AST

Parse a `func` expression into a nested `(operator, lhs, rhs)` tree of numbers
and variable symbols. Throws an `ArgumentError` on anything outside the grammar.
"""
function parse_func(func::AbstractString)
    p = FuncParser(func, tokenize(func), 1)
    ast = parse_expr(p)
    isnothing(current(p)) || throw(func_error(func, "unexpected $(repr(current(p)))"))
    return ast
end

evaluate(value::Float64, _) = value
evaluate(name::Symbol, variables) = variables[name]
evaluate((op, lhs, rhs)::Tuple, variables) = op.(evaluate(lhs, variables), evaluate(rhs, variables))

"""
    apply_func(func, x::AbstractArray) -> Array

Apply a `func` expression to every voxel of `x`. The statistics are taken over
the whole volume; `x_min`/`x_max` compare real parts of complex data.
"""
function apply_func(func::AbstractString, x::AbstractArray)
    ast = parse_func(func)
    variables = (;
        x,
        x_min=minimum(real, x),
        x_max=maximum(real, x),
        x_mean=mean(x),
        x_std=std(x; corrected=false),
    )
    result = evaluate(ast, variables)
    # An expression without `x` is a constant, but the map stays a map.
    return result isa AbstractArray ? result : fill(result, size(x))
end
