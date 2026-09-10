# mruby cross-build config for WASI (wasi-sdk)
# Usage: MRUBY_CONFIG=this_file.rb rake

WASI_SDK = 'C:/wasi-sdk'

MRuby::CrossBuild.new('wasm32-wasi') do |conf|
  toolchain :gcc

  conf.cc do |cc|
    cc.command = "#{WASI_SDK}/bin/clang"
    cc.flags = ['-O2', '-DWASM', '--target=wasm32-wasip1', '-mllvm', '-wasm-enable-sjlj']
  end

  conf.linker do |linker|
    linker.command = "#{WASI_SDK}/bin/clang"
    linker.flags = ['--target=wasm32-wasip1', '-mllvm', '-wasm-enable-sjlj', '-Wl,--no-entry', '-Wl,--export-all']
    linker.libraries << 'm'
  end

  conf.archiver do |archiver|
    archiver.command = "#{WASI_SDK}/bin/ar"
  end

  conf.cxx do |cxx|
    cxx.command = "#{WASI_SDK}/bin/clang++"
    cxx.flags = ['-O2', '-DWASM', '--target=wasm32-wasip1', '-mllvm', '-wasm-enable-sjlj']
  end

  # Core + compiler (needed for mrb_load_nstring eval)
  conf.gem :core => "mruby-compiler"
  conf.gem :core => "mruby-eval"
  conf.gem :core => "mruby-sprintf"
  conf.gem :core => "mruby-string-ext"
  conf.gem :core => "mruby-array-ext"
  conf.gem :core => "mruby-hash-ext"
  conf.gem :core => "mruby-enum-ext"
  conf.gem :core => "mruby-kernel-ext"

  conf.build_mrbtest_lib_only
end
