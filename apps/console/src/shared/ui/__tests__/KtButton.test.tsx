import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { KtButton } from '../KtButton'

describe('KtButton', () => {
  it('renders children', () => {
    render(<KtButton>Click me</KtButton>)
    expect(screen.getByRole('button', { name: /click me/i })).toBeInTheDocument()
  })

  it('fires onClick when clicked', () => {
    const handleClick = vi.fn()
    render(<KtButton onClick={handleClick}>Click</KtButton>)
    fireEvent.click(screen.getByRole('button'))
    expect(handleClick).toHaveBeenCalledTimes(1)
  })

  it('does not fire onClick when disabled', () => {
    const handleClick = vi.fn()
    render(
      <KtButton disabled onClick={handleClick}>
        Click
      </KtButton>
    )
    const btn = screen.getByRole('button')
    fireEvent.click(btn)
    expect(handleClick).not.toHaveBeenCalled()
    expect(btn).toBeDisabled()
  })

  it('shows loading indicator and disables button when loading', () => {
    const handleClick = vi.fn()
    render(
      <KtButton loading onClick={handleClick}>
        Submit
      </KtButton>
    )
    const btn = screen.getByRole('button')
    expect(btn).toBeDisabled()
    expect(screen.getByLabelText('loading')).toBeInTheDocument()
    fireEvent.click(btn)
    expect(handleClick).not.toHaveBeenCalled()
  })
})
